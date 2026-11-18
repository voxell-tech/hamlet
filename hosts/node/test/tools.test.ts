import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import { ok } from "@earendil-works/pi-durable/env";
import type { Exit } from "../src/agent/messages.ts";
import { remember } from "../src/agent/models.ts";
import {
    FAKE_ROOT,
    fakeCalls,
    fakeFiles,
    from,
    interrupted,
    named,
    timedOut,
    toolCall,
    toolResults,
    until,
    type Woken,
    wakeAgent,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

const ROOT = FAKE_ROOT;

async function results(agent: Woken): Promise<string[]> {
    return (await agent.entries())
        .filter((entry) => entry.kind === "pi.tool-result")
        .map((entry) => entry.text);
}

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
        [`${ROOT}/memory/plan.md`]: "own:plan",
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
            toolCall("read", { path: `${ROOT}/memory/plan.md` }, "c2"),
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
    assert.deepEqual(await results(agent), [
        "project:/work/project/notes.md",
        "own:plan",
    ]);
    await agent.stop();
});

test("a number the model gives past a u32's ends is held at them before it is sent", async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: {
            run: () => ({ type: "running", command: 4 }),
            read_file: () => ({ type: "text", text: "", notes: [] }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("bash", { command: "sleep 1", timeout: 1e7 }, "c1"),
            toolCall(
                "read",
                {
                    path: "/work/project/notes.md",
                    offset: 2 ** 32 + 5,
                    limit: -3,
                },
                "c2",
            ),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Run it and read on.", "task-1"),
    );
    await agent.idle();

    const [run] = named(sent, "run");
    const [read] = named(sent, "read_file");
    assert.ok(run);
    assert.equal(
        (run.message as { program: { timeout_ms: number } }).program.timeout_ms,
        2 ** 32 - 1,
    );
    assert.deepEqual(read?.message, {
        path: "/work/project/notes.md",
        offset: 2 ** 32 - 1,
        limit: 0,
    });
    await agent.stop();
});

test("a relative path, and a left-out one, start from the agent's filesystem", async (t) => {
    const { calls, sent } = fakeCalls({});
    const fake = fakeFiles({ [`${ROOT}/memory/plan.md`]: "own:plan" });
    const listed: string[] = [];
    const files = Object.assign(Object.create(fake) as typeof fake, {
        list: async (path: string) => {
            listed.push(path);
            return ok([
                {
                    name: "notes.md",
                    path: `${path}/notes.md`,
                    kind: "file" as const,
                    size: 5,
                },
            ]);
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        files,
        responses: [
            toolCall("read", { path: "memory/plan.md" }, "c1"),
            toolCall("find", {}, "c2"),
            fauxAssistantMessage("read and listed"),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Look."));
    await agent.idle();

    assert.deepEqual(
        sent
            .filter((call) => call.to === "workspace/local")
            .map((call) => call.name),
        ["list_tools"],
    );
    // The prompt lists `memory` and `skills` before each request.
    assert.deepEqual(
        listed.filter((path) => path !== "memory" && path !== "skills"),
        [ROOT],
    );
    assert.deepEqual(await results(agent), ["own:plan", "notes.md"]);
    await agent.stop();
});

test("a relative path starts from the project", {
    todo: "the file tools start a relative path from the agent's own filesystem (`own-files.ts` `start`)",
}, async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: {
            read_file: () => ({ type: "text", text: "notes", notes: [] }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        input: {
            kind: "hire",
            task: "standing-7",
            place: {
                version: 1,
                lead: 1,
                hires: [],
                roads: [],
                tools_off: [],
                folder: "/work",
            },
            copies: {},
        },
        responses: [
            toolCall("read", { path: "notes.md" }, "c1"),
            fauxAssistantMessage("Read it."),
        ],
    });
    t.after(() => agent.stop());
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Look."));
    await agent.idle();

    assert.deepEqual(
        named(sent, "read_file")
            .map(({ message }) => message as { path: string })
            .filter((read) => !/\/(AGENTS|CLAUDE)\.md$/.test(read.path)),
        [{ path: "/work/notes.md" }],
    );
});

test("a path that only begins with the root's name goes to the workspace", async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: {
            read_file: (message) => ({
                type: "text",
                text: `project:${message.path}`,
                notes: [],
            }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles({ [`${ROOT}-other/x`]: "own:x" }),
        responses: [
            toolCall("read", { path: `${ROOT}-other/x` }),
            fauxAssistantMessage("read"),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Read it."),
    );
    await agent.idle();

    assert.equal(named(sent, "read_file").length, 1);
    assert.deepEqual(await results(agent), [`project:${ROOT}-other/x`]);
    await agent.stop();
});

test("an own search gives the adapter its name, and a search or a find answers in today's words", async (t) => {
    const fake = fakeFiles({});
    const md = `${ROOT}/memory/plan.md`;
    const named: (string | undefined)[] = [];
    const files = Object.assign(Object.create(fake) as typeof fake, {
        search: async (
            _pattern: string,
            _path: string,
            options?: { name?: string },
        ) => {
            named.push(options?.name);
            return ok({
                found:
                    options?.name === "*.md"
                        ? [{ path: md, line: 1, text: "the plan" }]
                        : [],
                more: false,
                capped: false,
            });
        },
        find: async (glob: string) =>
            ok(
                glob === "*.md"
                    ? { found: [md], more: true, capped: false }
                    : { found: [], more: false, capped: true },
            ),
        list: async () =>
            ok([
                {
                    name: "b",
                    path: `${ROOT}/b`,
                    kind: "file" as const,
                    size: 1,
                },
                {
                    name: "a",
                    path: `${ROOT}/a`,
                    kind: "directory" as const,
                    size: 0,
                },
            ]),
    });
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        files,
        responses: [
            toolCall("search", { pattern: "plan", name: "*.md" }, "c1"),
            toolCall("search", { pattern: "plan", name: "*.rs" }, "c2"),
            toolCall("find", { name: "*.md" }, "c3"),
            toolCall("find", { name: "*.txt" }, "c4"),
            toolCall("find", { path: "memory" }, "c5"),
            fauxAssistantMessage("Found it."),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Find the plan."),
    );
    await agent.idle();

    assert.deepEqual(named, ["*.md", "*.rs"]);
    assert.deepEqual(await results(agent), [
        "memory/plan.md:1: the plan",
        "no match",
        "memory/plan.md\n[more than 100. Narrow it with `path` or `name`]",
        "no file by that name\n[stopped after 20000 files. Narrow it with `path`]",
        "a/\nb",
    ]);
    await agent.stop();
});

test("an own read answers as the workspace's read of the same file does", async (t) => {
    const lines = (count: number, width: number) =>
        Array.from({ length: count }, () => "x".repeat(width)).join("\n");
    const note = (code: string, message: string) => ({
        severity: "info" as const,
        code,
        message,
    });
    const png = Buffer.from(
        "89504e470d0a1a0a0000000d494844520000000100000001",
        "hex",
    );
    // Each read, and what the workspace answers for it
    // (`crates/hamlet_actors/src/workspace/files/read.rs`).
    const reads = [
        {
            file: lines(2500, 1),
            args: {},
            answer: {
                type: "text",
                text: lines(2000, 1),
                notes: [
                    note(
                        "truncated",
                        "Showing lines 1-2000 of 2500. Use offset=2001 to continue.",
                    ),
                ],
            },
        },
        {
            file: lines(100, 1000),
            args: {},
            answer: {
                type: "text",
                text: lines(51, 1000),
                notes: [
                    note(
                        "truncated",
                        "Showing lines 1-51 of 100 (50.0KB limit). Use offset=52 to continue.",
                    ),
                ],
            },
        },
        {
            file: lines(20, 1),
            args: { offset: 5, limit: 10 },
            answer: {
                type: "text",
                text: lines(10, 1),
                notes: [
                    note(
                        "",
                        "6 more lines in file. Use offset=15 to continue.",
                    ),
                ],
            },
        },
        {
            file: lines(20, 1),
            args: { offset: 30 },
            answer: {
                type: "failed",
                words: "Offset 30 is beyond end of file (20 lines total)",
            },
        },
        {
            file: "x".repeat(60_000),
            args: {},
            answer: {
                type: "text",
                text: "x".repeat(51_200),
                notes: [
                    {
                        severity: "warn",
                        code: "truncated",
                        // The note names the path the read was given.
                        message: `Line 1 is 58.6KB, exceeds the 50.0KB limit; showing its first 50.0KB. Use bash: sed -n '1p' ${ROOT}/4 | tail -c +51201`,
                    },
                ],
            },
        },
        {
            file: png.toString("latin1"),
            args: {},
            answer: {
                type: "image",
                mime: "image/png",
                data: png.toString("base64"),
            },
        },
    ];
    const answers = new Map(
        reads.map((read, at) => [`/work/${at}`, read.answer]),
    );
    const { calls } = fakeCalls({
        workspace: {
            read_file: (message) => answers.get(message.path) as never,
        },
    });
    const fake = fakeFiles({});
    const files = Object.assign(Object.create(fake) as typeof fake, {
        read: async (path: string) => {
            const at = Number(path.slice(`${ROOT}/`.length));
            return ok(
                new Uint8Array(Buffer.from(reads[at]?.file ?? "", "latin1")),
            );
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        files,
        responses: [
            ...reads.flatMap((read, at) => [
                toolCall(
                    "read",
                    { path: `/work/${at}`, ...read.args },
                    `w${at}`,
                ),
                toolCall(
                    "read",
                    { path: `${ROOT}/${at}`, ...read.args },
                    `o${at}`,
                ),
            ]),
            fauxAssistantMessage("Read."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Read."));
    await agent.idle();

    const taken = await results(agent);
    assert.equal(taken.length, reads.length * 2);
    for (const [at] of reads.entries()) {
        assert.equal(taken[at * 2 + 1], taken[at * 2], `read ${at}`);
    }
    await agent.stop();
});

test("an own write and an own edit answer in today's words, the edit with the workspace's diff", async (t) => {
    const { calls } = fakeCalls({});
    const path = `${ROOT}/memory/plan.md`;
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles({}),
        responses: [
            toolCall("write", { path, content: "a" }, "c1"),
            toolCall(
                "write",
                { path, content: "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n" },
                "c2",
            ),
            toolCall(
                "edit",
                {
                    path,
                    edits: [
                        { oldText: "f\n", newText: "F\n" },
                        { oldText: "k\n", newText: "k" },
                    ],
                },
                "c3",
            ),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Write."));
    await agent.idle();

    const wrote = {
        output: `Successfully wrote to ${path}`,
        isError: false,
        diagnostics: [],
    };
    assert.deepEqual(await toolResults(agent), [
        wrote,
        wrote,
        {
            output: `Successfully replaced 2 block(s) in ${path}.`,
            isError: false,
            diagnostics: [],
            // The diff and patch of `crates/hamlet_actors/src/workspace/files/diff.rs`'s test.
            details: {
                diff: "    ...\n  2 b\n  3 c\n  4 d\n  5 e\n- 6 f\n+ 6 F\n  7 g\n  8 h\n  9 i\n 10 j\n-11 k\n+11 k",
                patch: `--- ${path}\n+++ ${path}\n@@ -2,10 +2,10 @@\n b\n c\n d\n e\n-f\n+F\n g\n h\n i\n j\n-k\n+k\n\\ No newline at end of file\n`,
                firstChangedLine: 6,
            },
        },
    ]);
    await agent.stop();
});

test("an own write and edit name their path as today does, without `.` parts or doubled slashes", async (t) => {
    const { calls } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles({}),
        responses: [
            toolCall(
                "write",
                { path: "./memory//plan.md", content: "a\n" },
                "c1",
            ),
            toolCall(
                "edit",
                {
                    path: "memory/./plan.md",
                    edits: [{ oldText: "a", newText: "b" }],
                },
                "c2",
            ),
            fauxAssistantMessage("Done."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Write."));
    await agent.idle();

    const path = `${ROOT}/memory/plan.md`;
    const [wrote, edited] = await toolResults(agent);
    assert.deepEqual(wrote, {
        output: `Successfully wrote to ${path}`,
        isError: false,
        diagnostics: [],
    });
    assert.equal(
        edited?.output,
        `Successfully replaced 1 block(s) in ${path}.`,
    );
    const patch = (edited?.details as { patch: string } | undefined)?.patch;
    assert.equal(
        patch?.split("\n").slice(0, 2).join("\n"),
        `--- ${path}\n+++ ${path}`,
    );
    await agent.stop();
});

test("a path is own when it normalises into the agent's root, as today's `footing` decides", async (t) => {
    const own: Record<string, string> = {};
    const outside = "/realm/actors/agent/x";
    const { calls, sent } = fakeCalls({
        workspace: {
            write_file: () => ({
                type: "ended",
                end: {
                    type: "changed",
                    path: outside,
                    created: true,
                    hash: "1f",
                },
            }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(own),
        responses: [
            toolCall(
                "write",
                { path: `/tmp/..${ROOT}/memory/x`, content: "in" },
                "c1",
            ),
            toolCall(
                "write",
                { path: `${ROOT}/a/../../x`, content: "out" },
                "c2",
            ),
            fauxAssistantMessage("Written."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Write."));
    await agent.idle();

    const [inside, out] = await toolResults(agent);
    assert.equal(inside?.output, `Successfully wrote to ${ROOT}/memory/x`);
    assert.deepEqual(own, { [`${ROOT}/memory/x`]: "in" });
    assert.equal(out?.output, `Successfully wrote to ${outside}`);
    assert.deepEqual(
        named(sent, "write_file").map(
            (write) => (write.message as { path: string }).path,
        ),
        [outside],
    );
    await agent.stop();
});

test("bash refuses a timeout of 0 or below in today's words before it calls anything", async (t) => {
    const { calls, sent } = fakeCalls({});
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("bash", { command: "ls", timeout: 0 }, "c1"),
            toolCall("bash", { command: "ls", timeout: -1 }, "c2"),
            fauxAssistantMessage("Refused."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "List."));
    await agent.idle();

    const refusal = {
        output: "Error: Invalid timeout: must be a finite number of seconds. Inspect the output before trying again, or tell the chief.",
        isError: true,
        diagnostics: [],
    };
    assert.deepEqual(await toolResults(agent), [refusal, refusal]);
    assert.deepEqual(named(sent, "run"), []);
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
    assert.deepEqual(await results(agent), [
        "Command 4 is still running. Its end arrives as a new input.",
    ]);

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

test("a change whose call fails, a deadline included, says it was interrupted as today does, and is not sent again", async (t) => {
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
    assert.deepEqual(await toolResults(agent), [interrupted("bash")]);
    await agent.stop();
});

test("an own edit that a cut-off interrupts is not run again, and says it was interrupted as today does", async (t) => {
    const path = `${ROOT}/memory/plan.md`;
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
            return done;
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
    assert.deepEqual(await toolResults(second), [interrupted("edit")]);
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

const ended = (
    command: number,
    exit: Exit,
    output: string,
    cut?: { full_output: string },
) => ({
    type: "ended" as const,
    end: {
        type: "ran" as const,
        command,
        exit,
        output,
        ...(cut === undefined
            ? {}
            : {
                  cut: {
                      total_bytes: 90_000,
                      total_lines: 3_000,
                      full_output: cut.full_output,
                  },
              }),
        policy: "3f2a",
        took_ms: 10,
    },
});

test("a command that ends inside its call answers as today's bash does", async (t) => {
    const ends: Record<string, ReturnType<typeof ended>> = {
        "echo hi": ended(1, { type: "code", code: 0 }, "hi\n"),
        make: ended(2, { type: "code", code: 2 }, "tail\n", {
            full_output: "/tmp/hamlet/out-2.log",
        }),
        "sleep 200": ended(3, { type: "timed_out" }, "a\n"),
        "sleep 500": ended(4, { type: "timed_out" }, ""),
        nope: ended(5, { type: "failed", words: "spawn failed" }, ""),
        kill: ended(6, { type: "cut_off" }, "half\n"),
        stop: ended(7, { type: "cancelled" }, "x\n"),
        "touch /x": ended(
            8,
            { type: "code", code: 1 },
            "touch: cannot touch '/x': Permission denied\n",
        ),
        "cat /x": ended(9, { type: "code", code: 0 }, "Permission denied\n"),
    };
    const { calls } = fakeCalls({
        workspace: {
            run: (message) => {
                const { program } = message;
                assert.equal(program.type, "script");
                const end =
                    program.type === "script"
                        ? ends[program.script]
                        : undefined;
                assert.ok(end);
                return end;
            },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("bash", { command: "echo hi" }, "c1"),
            toolCall("bash", { command: "make" }, "c2"),
            toolCall("bash", { command: "sleep 200", timeout: 1.5 }, "c3"),
            toolCall("bash", { command: "sleep 500", timeout: 600 }, "c4"),
            toolCall("bash", { command: "nope" }, "c5"),
            toolCall("bash", { command: "kill" }, "c6"),
            toolCall("bash", { command: "stop" }, "c7"),
            toolCall("bash", { command: "touch /x" }, "c8"),
            toolCall("bash", { command: "cat /x" }, "c9"),
            fauxAssistantMessage("Ran."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Run."));
    await agent.idle();

    assert.deepEqual(await toolResults(agent), [
        { output: "hi\n", isError: false, diagnostics: [] },
        {
            output: "tail\n",
            isError: true,
            diagnostics: [
                {
                    severity: "info",
                    code: "full_output",
                    message: "Full output: /tmp/hamlet/out-2.log",
                },
                {
                    severity: "error",
                    code: "exit_code",
                    message: "Command exited with code 2",
                },
            ],
        },
        {
            output: "a\n\n[timed out after 1.5s]",
            isError: true,
            diagnostics: [],
        },
        {
            output: "\n[timed out after 120s]",
            isError: true,
            diagnostics: [],
        },
        {
            output: "Error: spawn failed. Inspect the output before trying again, or tell the chief.",
            isError: true,
            diagnostics: [],
        },
        interrupted("bash"),
        // Today has no words for a cancelled command.
        {
            output: "x\n\n[Command 7: cancelled]",
            isError: true,
            diagnostics: [],
        },
        {
            output: "touch: cannot touch '/x': Permission denied\n\n[The sandbox lets a command write only inside the workspace, which is its working directory. Use a relative path there, not /tmp or another folder.]",
            isError: true,
            diagnostics: [
                {
                    severity: "error",
                    code: "exit_code",
                    message: "Command exited with code 1",
                },
            ],
        },
        { output: "Permission denied\n", isError: false, diagnostics: [] },
    ]);
    await agent.stop();
});

test("a hand tool that ends inside its call answers as today's hand tools do", async (t) => {
    const ends: Record<string, ReturnType<typeof ended>> = {
        ok: ended(1, { type: "code", code: 0 }, "3 words\n"),
        bad: ended(2, { type: "code", code: 1 }, "no file\n"),
        spawn: ended(3, { type: "failed", words: "exec format error" }, ""),
        kill: ended(4, { type: "cut_off" }, ""),
        long: ended(5, { type: "code", code: 0 }, "tail\n", {
            full_output: "/tmp/hamlet/out-5.log",
        }),
    };
    const { calls } = fakeCalls({
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
            run: (message) => {
                const { program } = message;
                assert.equal(program.type, "tool");
                const end =
                    program.type === "tool"
                        ? ends[
                              (
                                  JSON.parse(program.arguments) as {
                                      path: string;
                                  }
                              ).path
                          ]
                        : undefined;
                assert.ok(end);
                return end;
            },
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("word_count", { path: "ok" }, "c1"),
            toolCall("word_count", { path: "bad" }, "c2"),
            toolCall("word_count", { path: "spawn" }, "c3"),
            toolCall("word_count", { path: "kill" }, "c4"),
            toolCall("word_count", { path: "long" }, "c5"),
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
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Count."));
    await agent.idle();

    assert.deepEqual(await toolResults(agent), [
        { output: "3 words\n", isError: false, diagnostics: [] },
        {
            output: "no file\n",
            isError: true,
            diagnostics: [
                {
                    severity: "error",
                    code: "exit_code",
                    message: "Command exited with code 1",
                },
            ],
        },
        {
            output: "",
            isError: true,
            diagnostics: [
                {
                    severity: "error",
                    code: "tool_error",
                    message: "exec format error",
                },
            ],
        },
        interrupted("word_count"),
        { output: "tail\n", isError: false, diagnostics: [] },
    ]);
    await agent.stop();
});

test("a workspace write and edit answer in today's words, in the call and later", async (t) => {
    const diff = {
        diff: "- 1 a\n+ 1 A",
        patch: "--- /work/a.md\n+++ /work/a.md\n@@ -1,1 +1,1 @@\n-a\n+A\n",
        first_changed_line: 1,
    };
    const { calls, sent } = fakeCalls({
        workspace: {
            write_file: (message) =>
                message.path === "/work/a.md"
                    ? {
                          type: "ended",
                          end: {
                              type: "changed",
                              path: "/work/a.md",
                              created: true,
                              hash: "1f",
                          },
                      }
                    : { type: "waits_for_chief", needs: ["edit"] },
            edit_file: () => ({
                type: "ended",
                end: {
                    type: "changed",
                    path: "/work/a.md",
                    created: false,
                    hash: "2f",
                    diff,
                },
            }),
        },
    });
    const agent = await wakeAgent(t, {
        calls,
        responses: [
            toolCall("write", { path: "/work/a.md", content: "a\n" }, "c1"),
            toolCall(
                "edit",
                {
                    path: "/work/a.md",
                    edits: [
                        { oldText: "a", newText: "A" },
                        { oldText: "b", newText: "B" },
                    ],
                },
                "c2",
            ),
            toolCall("write", { path: "/work/b.md", content: "b" }, "c3"),
            fauxAssistantMessage("Wrote; one waits."),
            fauxAssistantMessage("It was written."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Write."));
    await agent.idle();

    assert.deepEqual((await toolResults(agent)).slice(0, 2), [
        {
            output: "Successfully wrote to /work/a.md",
            isError: false,
            diagnostics: [],
        },
        {
            output: "Successfully replaced 2 block(s) in /work/a.md.",
            isError: false,
            diagnostics: [],
            details: {
                diff: diff.diff,
                patch: diff.patch,
                firstChangedLine: 1,
            },
        },
    ]);

    const [, waited] = named(sent, "write_file");
    assert.ok(waited);
    assert.equal(
        await agent.actions.finished(agent.by(from.workspace), {
            call: (waited.message as { call: string }).call,
            outcome: {
                type: "change",
                end: {
                    type: "changed",
                    path: "/work/b.md",
                    created: true,
                    hash: "3f",
                },
            },
        }),
        null,
    );
    await agent.idle();
    assert.deepEqual(
        (await agent.entries())
            .filter((entry) => entry.kind === "hamlet.finished")
            .map((entry) => entry.text),
        ["Successfully wrote to /work/b.md"],
    );
    await agent.stop();
});

// As today scrubs every tool's result (C32,
// `crates/hamlet_server/src/work/tools.rs:567-570,606`).
test("an own file tool's result holds no secret the host knows", async (t) => {
    const key = "own-file-key-4f1c9e";
    remember(key);
    const files = fakeFiles({ [`${ROOT}/notes.env`]: `TOKEN=${key}\n` });
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        files,
        responses: [
            toolCall("read", { path: `${ROOT}/notes.env` }),
            toolCall("edit", {
                path: `${ROOT}/notes.env`,
                edits: [{ oldText: "TOKEN", newText: "KEY" }],
            }),
            fauxAssistantMessage("Done."),
        ],
    });
    t.after(() => agent.stop());
    await agent.actions.submit(agent.by(from.realm), line("in-1", "Look"));
    await agent.idle();

    const kept = await results(agent);
    assert.equal(kept.length, 2);
    assert.ok(kept[0]?.includes("TOKEN=[REDACTED]"), kept[0]);
    assert.ok(
        !JSON.stringify(await agent.entries()).includes(key),
        "no entry holds the key",
    );
});
