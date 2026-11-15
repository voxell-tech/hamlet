/**
 * U9: the tools that reach the workspace, and the agent's own file routes.
 */
import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import { ok } from "@earendil-works/pi-durable/env";
import {
    fakeCalls,
    fakeFiles,
    from,
    named,
    timedOut,
    toolCall,
    until,
    wakeAgent,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

const line = (id: string, text: string, task?: string) => ({
    id,
    ...(task === undefined ? {} : { task }),
    input: { type: "line" as const, text },
});

test("a project path goes to the workspace and an own path to the adapter", async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: {
            read_file: (message) => ({
                type: "text",
                text: `project:${message.path}`,
                notes: [],
            }),
        },
    });
    const files = fakeFiles({
        "/realm/actors/agent/7.fs/memory/plan.md": "own:plan",
    });
    const agent = await wakeAgent(t, {
        calls,
        files,
        responses: [
            toolCall(
                "read",
                { path: "/work/project/notes.md", limit: 10 },
                "c1",
            ),
            toolCall(
                "read",
                { path: "/realm/actors/agent/7.fs/memory/plan.md" },
                "c2",
            ),
            fauxAssistantMessage("read both"),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Read them."),
    );
    await agent.idle();

    assert.deepEqual(named(sent, "read_file"), [
        {
            to: "workspace/local",
            name: "read_file",
            // A read carries no id, and an unset field is absent.
            message: { path: "/work/project/notes.md", limit: 10 },
            deadlineMs: 15_000,
        },
    ]);
    const results = (await agent.entries()).filter(
        (entry) => entry.kind === "pi.tool-result",
    );
    assert.deepEqual(
        results.map((entry) => entry.text),
        ["project:/work/project/notes.md", "own:plan"],
    );
    await agent.stop();
});

test("an own search keeps only the files whose name matches", async (t) => {
    const fake = fakeFiles({});
    const md = `${fake.root}/memory/plan.md`;
    const txt = `${fake.root}/memory/plan.txt`;
    const files = Object.assign(Object.create(fake) as typeof fake, {
        search: async () =>
            ok({
                found: [md, txt].map((path) => ({
                    path,
                    line: 1,
                    text: "the plan",
                })),
                capped: false,
            }),
        find: async (glob: string) =>
            ok({ found: glob === "*.md" ? [md] : [], capped: false }),
    });
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        files,
        responses: [
            toolCall("search", {
                pattern: "plan",
                path: files.root,
                name: "*.md",
            }),
            fauxAssistantMessage("Found it."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Find the plan."),
    );
    await agent.idle();

    assert.equal(
        (await agent.entries()).find((entry) => entry.kind === "pi.tool-result")
            ?.text,
        `${md}:1: the plan`,
    );
    await agent.stop();
});

test("running ends the tool, not the turn, and the end is taken once under its call id", async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: { run: () => ({ type: "running", command: 4 }) },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("bash", { command: "cargo test", timeout: 60 }),
            fauxAssistantMessage("It runs; I will hear its end."),
            fauxAssistantMessage("The tests failed."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Run the tests.", "task-1"),
    );
    await agent.idle();

    // The call carries its task's id, its conversation and the turn's task.
    const [run] = named(sent, "run");
    assert.ok(run);
    const message = run.message as { call: string; conversation: string };
    assert.match(message.call, /^t[1-9][0-9]*$/);
    assert.deepEqual(run, {
        to: "workspace/local",
        name: "run",
        message: {
            call: message.call,
            conversation: message.conversation,
            realm_task: "task-1",
            program: {
                type: "script",
                script: "cargo test",
                network: false,
                timeout_ms: 60_000,
            },
        },
        deadlineMs: 15_000,
    });
    // The tool ended with the command still running, and the turn went on to its answer.
    assert.equal(agent.faux.state.callCount, 2);
    assert.equal(
        (await agent.entries()).find((entry) => entry.kind === "pi.tool-result")
            ?.text,
        "Command 4 is still running. Its end arrives as a new input.",
    );

    // Later the workspace tells the agent the end, and tells it again.
    const end = {
        call: message.call,
        outcome: {
            type: "run" as const,
            end: {
                type: "ran" as const,
                command: 4,
                exit: { type: "code" as const, code: 1 },
                output: "2 failed\n",
                policy: "3f2a",
                took_ms: 12_000,
            },
        },
    };
    assert.equal(
        await agent.actions.finished(agent.by(from.workspace), end),
        null,
    );
    assert.equal(
        await agent.actions.finished(agent.by(from.workspace), end),
        null,
    );
    await agent.idle();

    assert.deepEqual(
        (await agent.entries())
            .filter((entry) => entry.kind === "hamlet.finished")
            .map((entry) => entry.text),
        ["2 failed\n\n[Command 4: exit code 1]"],
    );
    // One more turn ran on the end: three requests in all.
    assert.equal(agent.faux.state.callCount, 3);
    await agent.stop();
});

test("a change from a turn whose input named no task carries no realm_task", async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: {
            write_file: () => ({
                type: "ended",
                end: {
                    type: "changed",
                    path: "/work/a.md",
                    created: true,
                    hash: "1f",
                },
            }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("write", { path: "/work/a.md", content: "a" }),
            fauxAssistantMessage("Written."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Write it."),
    );
    await agent.idle();

    const [write] = named(sent, "write_file");
    const message = write?.message as { call: string; conversation: string };
    assert.deepEqual(message, {
        call: message.call,
        conversation: message.conversation,
        path: "/work/a.md",
        content: "a",
    });
    await agent.stop();
});

test("a rerun after a cut-off sends the same id", async (t) => {
    const hung = Promise.withResolvers<void>();
    let runs = 0;
    const { calls, sent } = fakeCalls({
        workspace: {
            run: async () => {
                runs += 1;
                if (runs === 1) {
                    hung.resolve();
                    // The first call never answers: its wake is cut off under it.
                    await new Promise(() => {});
                }
                return { type: "running" as const, command: 4 };
            },
        },
    });
    const first = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("bash", { command: "sleep 30" }),
            fauxAssistantMessage("It runs."),
        ],
    });
    await first.actions.submit(
        first.by(from.realm),
        line("input-1", "Run it."),
    );
    await hung.promise;
    await first.stop();

    const second = await first.again();
    await second.idle();

    const ids = named(sent, "run").map(
        (call) => (call.message as { call: string }).call,
    );
    assert.equal(ids.length, 2);
    assert.equal(ids[0], ids[1]);
    await second.stop();
});

test("a call that fails says its outcome is unknown, and is not sent again", async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: {
            run: () => {
                throw timedOut();
            },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("bash", { command: "make install" }),
            fauxAssistantMessage("I will look first."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Install it."),
    );
    await agent.idle();

    assert.equal(named(sent, "run").length, 1);
    assert.equal(
        (await agent.entries()).find((entry) => entry.kind === "pi.tool-result")
            ?.text,
        "`run` was cut off: actor.action_timed_out: The action timed out.. Its outcome is unknown: it may have been carried out or not, and nothing sends it again. Look at what it would have changed before you repeat it.",
    );
    await agent.stop();
});

test("an own edit that a cut-off interrupts is not run again, and says its outcome is unknown", async (t) => {
    const path = "/realm/actors/agent/7.fs/memory/plan.md";
    const files = fakeFiles({ [path]: "one two" });
    const cut = Promise.withResolvers<void>();
    // The first edit is made, and then the wake is cut off before it answers.
    const edit = files.edit.bind(files);
    let edits = 0;
    const hanging = Object.assign(Object.create(files) as typeof files, {
        edit: async (...args: Parameters<typeof edit>) => {
            edits += 1;
            const done = await edit(...args);
            if (edits > 1) return done;
            cut.resolve();
            const signal = args[2].abortSignal;
            await new Promise((_, reject) =>
                signal?.addEventListener("abort", () =>
                    reject(new Error("cut off")),
                ),
            );
            return ok(undefined);
        },
    });
    const { calls } = fakeCalls({});
    const first = await wakeAgent(t, {
        calls,
        files: hanging,
        responses: [
            toolCall("edit", {
                path,
                edits: [{ oldText: "one", newText: "three" }],
            }),
            fauxAssistantMessage("I will read it."),
        ],
    });
    await first.actions.submit(
        first.by(from.realm),
        line("input-1", "Edit it."),
    );
    await cut.promise;
    await first.stop();

    const second = await first.again();
    await second.idle();

    // The adapter was asked once: the rerun found the mark and did not edit.
    assert.equal(edits, 1);
    assert.equal(files.edits, 1);
    assert.equal(
        (await second.entries()).find(
            (entry) => entry.kind === "pi.tool-result",
        )?.text,
        `The edit of ${path} was cut off: the agent stopped while it ran. Its outcome is unknown: it may have been carried out or not, and nothing sends it again. Look at what it would have changed before you repeat it.`,
    );
    await second.stop();
});

test("a tool a chief added by hand is listed after the wake and runs under its call id", async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: {
            list_tools: () => ({
                tools: [
                    {
                        name: "word_count",
                        description: "Count the words of a file.",
                        parameters: JSON.stringify({
                            type: "object",
                            required: ["path"],
                            properties: { path: { type: "string" } },
                        }),
                    },
                ],
                faults: [],
            }),
            run: () => ({ type: "running", command: 9 }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("word_count", { path: "/project/a.md" }),
            fauxAssistantMessage("Counted."),
        ],
    });
    const harness = await agent.harness();
    const root = await harness.root(context);
    await until(async () =>
        (await root.agent(context)).tools.some(
            (tool) => tool.name === "word_count",
        ),
    );
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Count.", "task-1"),
    );
    await agent.idle();

    const [run] = named(sent, "run");
    assert.ok(run);
    const message = run.message as { call: string; conversation: string };
    assert.deepEqual(run.message, {
        call: message.call,
        conversation: message.conversation,
        realm_task: "task-1",
        program: {
            type: "tool",
            name: "word_count",
            arguments: '{"path":"/project/a.md"}',
        },
    });
    assert.match(message.call, /^t[1-9][0-9]*$/);
    await agent.stop();
});
