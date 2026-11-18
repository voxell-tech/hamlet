import assert from "node:assert/strict";
import { type TestContext, test } from "node:test";
import {
    type Context,
    type FauxResponseFactory,
    fauxAssistantMessage,
} from "@earendil-works/pi-ai";
import { getCurrentSystemPrompt } from "@earendil-works/pi-ai/utils/transcript";
import type {
    AgentInput,
    ChatEvent,
    ChatItem,
    Copies,
    Place,
    ReadAnswer,
    Submit,
} from "../src/agent/messages.ts";
import {
    fakeCalls,
    fakeFiles,
    from,
    named,
    timedOut,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context, fixture } from "./testing.ts";

const ERRAND = fixture("agent/errand.input") as AgentInput;
const WORKER = fixture("agent/worker.input") as AgentInput;

function asked(seen: Context[], words: string): FauxResponseFactory {
    return (context) => {
        seen.push(context);
        return fauxAssistantMessage(words);
    };
}

async function offered(agent: Woken): Promise<string[]> {
    const root = await (await agent.harness()).root(context);
    return (await root.agent(context)).tools.map((tool) => tool.name).sort();
}

function userWords(context: Context): string[] {
    return context.messages.flatMap((message) =>
        message.role !== "user"
            ? []
            : typeof message.content === "string"
              ? [message.content]
              : message.content.flatMap((part) =>
                    part.type === "text" ? [part.text] : [],
                ),
    );
}

test("an errand answers once and ends", async (t) => {
    const { calls, sent } = fakeCalls({ realm: { errand_done: () => null } });
    const agent = await wakeAgent(t, {
        calls,
        input: ERRAND,
        responses: [fauxAssistantMessage("PASS: fine")],
    });
    t.after(() => agent.stop());
    await until(() => named(sent, "errand_done").length === 1);
    await agent.idle();

    assert.deepEqual(
        named(sent, "errand_done").map(({ to, message }) => [to, message]),
        [["realm/local", { ended: { type: "answered", text: "PASS: fine" } }]],
    );
    assert.deepEqual(named(sent, "worker_done"), []);
    assert.equal(agent.faux.state.callCount, 1);
    assert.deepEqual(agent.reported, []);
});

test("a worker's turn ends with worker_done, once after a kill", async (t) => {
    let cut = true;
    const { calls, sent } = fakeCalls({
        realm: {
            worker_done: () => {
                if (!cut) return null;
                cut = false;
                throw timedOut();
            },
        },
    });
    const first = await wakeAgent(t, {
        calls,
        input: WORKER,
        responses: [fauxAssistantMessage("  Two links are broken.\n")],
    });
    await until(() => first.reported.length === 1);
    await first.idle();
    assert.equal(named(sent, "worker_done").length, 1);
    await first.stop();

    const again = await first.again();
    t.after(() => again.stop());
    await until(() => named(sent, "worker_done").length === 2);
    await again.idle();

    const answered = {
        ended: { type: "answered", text: "Two links are broken." },
    };
    assert.deepEqual(
        named(sent, "worker_done").map(({ message }) => message),
        [answered, answered],
    );
    assert.equal(first.faux.state.callCount, 1);
    assert.deepEqual(
        (await again.entries())
            .filter((entry) => entry.kind.startsWith("hamlet."))
            .map(({ kind, text }) => [kind, text]),
        [["hamlet.worker", "Check the links in the docs"]],
    );
    assert.deepEqual(named(sent, "errand_done"), []);
    assert.deepEqual(again.reported, []);
});

test("an errand's prompt is its instructions only", async (t) => {
    const seen: Context[] = [];
    const { calls } = fakeCalls({ realm: { errand_done: () => null } });
    const agent = await wakeAgent(t, {
        calls,
        input: ERRAND,
        responses: [asked(seen, "PASS")],
    });
    t.after(() => agent.stop());
    await until(() => seen.length === 1);
    await agent.idle();

    const [request] = seen;
    assert.ok(request !== undefined);
    assert.equal(
        getCurrentSystemPrompt(request.messages),
        "Review the chief's line.",
    );
    assert.deepEqual(userWords(request), ["Write the tests"]);
    // As today (`calls/place.rs:232`), an errand is offered no tool.
    assert.deepEqual(await offered(agent), []);
});

test("a worker is offered only the built-in tools its brief names", async (t) => {
    const added = (name: string) => ({
        name,
        description: `Runs ${name}.`,
        parameters: JSON.stringify({ type: "object", properties: {} }),
    });
    const { calls, sent } = fakeCalls({
        workspace: {
            list_tools: () => ({
                tools: [added("lint"), added("deploy")],
                faults: [],
            }),
        },
        realm: { worker_done: () => null },
    });
    const brief = WORKER.brief;
    assert.ok(brief !== undefined);
    const agent = await wakeAgent(t, {
        calls,
        input: {
            ...WORKER,
            brief: { ...brief, tools: [{ name: "read" }, { name: "lint" }] },
        },
        responses: [fauxAssistantMessage("Done.")],
    });
    t.after(() => agent.stop());
    await until(() => named(sent, "worker_done").length === 1);
    // The tools a workspace lists arrive after the wake.
    await until(async () => (await offered(agent)).includes("lint"));

    // As today (`calls/place.rs:189-197`): no society tool, no timer.
    assert.deepEqual(await offered(agent), ["lint", "read"]);
});

test("a worker whose brief names no tool is offered every built-in tool", async (t) => {
    const { calls, sent } = fakeCalls({ realm: { worker_done: () => null } });
    const brief = WORKER.brief;
    assert.ok(brief !== undefined);
    const agent = await wakeAgent(t, {
        calls,
        input: { ...WORKER, brief: { text: brief.text } },
        responses: [fauxAssistantMessage("Done.")],
    });
    t.after(() => agent.stop());
    await until(() => named(sent, "worker_done").length === 1);

    assert.deepEqual(await offered(agent), [
        "bash",
        "edit",
        "find",
        "read",
        "search",
        "write",
    ]);
});

test("a place sent to an errand offers it no tool", async (t) => {
    const copies = Promise.withResolvers<Copies>();
    const { calls, sent } = fakeCalls({
        realm: { read_copies: () => copies.promise, errand_done: () => null },
    });
    const agent = await wakeAgent(t, {
        calls,
        input: ERRAND,
        responses: [fauxAssistantMessage("PASS")],
    });
    t.after(() => agent.stop());
    await until(() => named(sent, "read_copies").length === 1);
    const place: Place = { version: 2, hires: [], roads: [], tools_off: [] };
    await agent.actions.place(agent.by(from.realm), place);
    copies.resolve({});
    await until(() => named(sent, "errand_done").length === 1);

    assert.deepEqual(await offered(agent), []);
});

test("a notice, a worker's answer and a reviewed line's note are taken and shown as today", async (t) => {
    const seen: Context[] = [];
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            asked(seen, "On it."),
            asked(seen, "Using staging."),
            asked(seen, "Fixing them."),
        ],
    });
    t.after(() => agent.stop());
    const submit = async (name: string, id: string) => {
        const [message] = fixture(name) as [Submit];
        await agent.actions.submit(agent.by(from.realm), { ...message, id });
        await agent.idle();
    };
    await submit("agent/submit.line.reviewed.args", "input-1");
    await submit("agent/submit.notice.args", "input-2");
    await submit("agent/submit.worker_answer.args", "input-3");

    assert.deepEqual(
        seen.map((request) => userWords(request).at(-1)),
        [
            "Write the tests\n\nhire#2 reviewed this line: fine",
            "The chief answers: use the staging server",
            // As `crates/hamlet_server/src/talk/letters.rs:334` words it.
            "worker#15 answers: Three links are broken",
        ],
    );
    const page = await agent.actions.read_chat(agent.by(from.client), {
        id: "11111111-1111-4111-8111-111111111111",
        chat: { with: 7 },
    });
    const shown = (item: ChatItem) => ({
        kind: item.kind,
        text: item.text,
        ...(item.author === undefined ? {} : { author: item.author }),
        ...(item.rewindable === undefined
            ? {}
            : { rewindable: item.rewindable }),
    });
    // As `crates/hamlet_server/src/clients/fold.rs:296-340` shows a chief's
    // line, a notice and a reply.
    assert.deepEqual(page.items.map(shown), [
        { kind: "user", text: "Write the tests", rewindable: true },
        { kind: "notice", text: "hire#2 reviewed this line: fine" },
        { kind: "assistant", text: "On it." },
        { kind: "notice", text: "The chief answers: use the staging server" },
        { kind: "assistant", text: "Using staging." },
        {
            kind: "user",
            text: "worker#15 answers: Three links are broken",
            author: "worker#15",
        },
        { kind: "assistant", text: "Fixing them." },
    ]);
    const inputs = (items: readonly ChatItem[]) =>
        items.filter((item) => item.kind !== "assistant").map(shown);
    const added = agent.events
        .filter(([name]) => name === "chat")
        .flatMap(([, event]) => (event as ChatEvent).ops)
        .flatMap((op) => (op.type === "add" ? [op.item] : []));
    assert.deepEqual(inputs(added), inputs(page.items));
    assert.deepEqual(agent.reported, []);
});

test("an errand that answers no words fails with today's words", async (t) => {
    const { calls, sent } = fakeCalls({ realm: { errand_done: () => null } });
    const agent = await wakeAgent(t, {
        calls,
        input: ERRAND,
        responses: [fauxAssistantMessage(" ")],
    });
    t.after(() => agent.stop());
    await until(() => named(sent, "errand_done").length === 1);

    // `crates/hamlet_server/src/talk/errands.rs:133`.
    assert.deepEqual(named(sent, "errand_done")[0]?.message, {
        ended: { type: "failed", why: "the errand gave no answer" },
    });
});

test("a worker cut off with no words ends with the notice's words", async (t) => {
    const { calls, sent } = fakeCalls({ realm: { worker_done: () => null } });
    const agent = await wakeAgent(t, {
        calls,
        input: WORKER,
        responses: [fauxAssistantMessage([], { stopReason: "length" })],
    });
    t.after(() => agent.stop());
    await until(() => named(sent, "worker_done").length === 1);

    // As today (`talk/follow.rs:73-78`), a failed turn ends with its reason.
    assert.deepEqual(named(sent, "worker_done")[0]?.message, {
        ended: {
            type: "failed",
            why: "the message is longer than the model can hold",
        },
    });
    // Only a chief's turn opens a wait (`talk/follow.rs:54-59`).
    assert.deepEqual(named(sent, "turn_failed"), []);
});

test("a worker's turn runs on the charter its create input holds, while the realm has not answered its read", async (t) => {
    const seen: Context[] = [];
    const copies = Promise.withResolvers<Copies>();
    const { calls, sent } = fakeCalls({
        realm: {
            read_copies: () => copies.promise,
            worker_done: () => null,
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        input: {
            ...WORKER,
            copies: {
                charter: { version: 1, value: "Keep the docs whole." },
            },
        },
        responses: [asked(seen, "Done.")],
    });
    t.after(() => {
        copies.resolve({});
        return agent.stop();
    });
    await until(() => named(sent, "worker_done").length === 1);

    const [request] = seen;
    assert.ok(request !== undefined);
    assert.match(
        getCurrentSystemPrompt(request.messages),
        /Keep the docs whole\./,
    );
});

async function workerPrompt(t: TestContext): Promise<string> {
    const seen: Context[] = [];
    const { calls, sent } = fakeCalls({
        workspace: {
            read_file: ({ path }: { path: string }): ReadAnswer =>
                path === "/work/AGENTS.md"
                    ? { type: "text", text: "Use tabs.", notes: [] }
                    : { type: "failed", words: `${path} is not there` },
        },
        realm: {
            read_copies: (): Copies => ({
                charter: { version: 1, value: "Keep the docs whole." },
                character: { version: 1, value: "Calm." },
            }),
            worker_done: () => null,
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        input: { ...WORKER, place: { ...WORKER.place, folder: "/work" } },
        files: fakeFiles({ "memory/chief.md": "The chief likes tea." }),
        responses: [asked(seen, "Done.")],
    });
    t.after(() => agent.stop());
    await until(() => named(sent, "worker_done").length === 1);
    const [request] = seen;
    assert.ok(request !== undefined);
    return getCurrentSystemPrompt(request.messages);
}

// Today's worker reads the charter, the machine, the project and its place,
// and no memory (`talk/prompt.rs:78-98`, `talk/prompt/place.rs:84-94`).
test("a worker's prompt is the charter, the environment, the project and today's worker place", async (t) => {
    const prompt = await workerPrompt(t);

    assert.ok(prompt.startsWith("Keep the docs whole.\n\n# Environment\n"));
    assert.deepEqual(
        prompt.split("\n").filter((line) => line.startsWith("# ")),
        [
            "# Environment",
            "# The project",
            "# The project's AGENTS.md",
            "# Your place",
        ],
    );
    assert.ok(
        prompt.endsWith(
            "\n\n# Your place\nYou are worker#7, started by a chief for one task, given as your first message. Do it, then answer in your final message: that answer is all a chief gets. Your lodge is a scratch folder that ends with you; you keep no memory.",
        ),
    );
});

test("a worker's prompt names the agent that started it", {
    todo: "no message carries a worker's starter, which `talk/prompt/place.rs:85-88` names",
}, async (t) => {
    assert.match(await workerPrompt(t), /started by (hand|hire#\d+)/);
});
