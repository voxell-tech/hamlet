import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import { ok } from "@earendil-works/pi-durable/env";
import {
    FAKE_ROOT,
    fakeCalls,
    fakeFiles,
    from,
    line,
    named,
    resultTexts,
    toolCall,
    toolResults,
    wakeAgent,
} from "./agent-testing.ts";

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
        [`${FAKE_ROOT}/memory/plan.md`]: "own:plan",
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
            toolCall("read", { path: `${FAKE_ROOT}/memory/plan.md` }, "c2"),
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
    assert.deepEqual(await resultTexts(agent), [
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
    const fake = fakeFiles({ [`${FAKE_ROOT}/memory/plan.md`]: "own:plan" });
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
        [FAKE_ROOT],
    );
    assert.deepEqual(await resultTexts(agent), ["own:plan", "notes.md"]);
    await agent.stop();
});

test("a relative path, and a left-out one, start from the project", async (t) => {
    const { calls, sent } = fakeCalls({
        workspace: {
            read_file: () => ({ type: "text", text: "notes", notes: [] }),
            search: () => ({ type: "text", text: "no match", notes: [] }),
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
                manual: false,
                agrees_roads: false,
            },
            copies: {},
        },
        responses: [
            toolCall("read", { path: "notes.md" }, "c1"),
            toolCall("search", { pattern: "TODO" }, "c2"),
            fauxAssistantMessage("Read it."),
        ],
    });
    t.after(() => agent.stop());
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Look."));
    await agent.idle();

    assert.deepEqual(
        named(sent, "read_file")
            .map(({ message }) => message as { path: string })
            // The prompt reads the project's instructions and its `.git`.
            .filter(
                (read) =>
                    !/\/(AGENTS\.md|CLAUDE\.md|\.git|\.git\/HEAD)$/.test(
                        read.path,
                    ),
            ),
        [{ path: "/work/notes.md" }],
    );
    assert.deepEqual(
        named(sent, "search").map(
            ({ message }) => (message as { under: string }).under,
        ),
        ["/work"],
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
        files: fakeFiles({ [`${FAKE_ROOT}-other/x`]: "own:x" }),
        responses: [
            toolCall("read", { path: `${FAKE_ROOT}-other/x` }),
            fauxAssistantMessage("read"),
        ],
    });
    await agent.actions.submit(
        agent.by(from.realm),
        line("input-1", "Read it."),
    );
    await agent.idle();

    assert.equal(named(sent, "read_file").length, 1);
    assert.deepEqual(await resultTexts(agent), [
        `project:${FAKE_ROOT}-other/x`,
    ]);
    await agent.stop();
});

test("an own search gives the adapter its name, and a search or a find answers in its words", async (t) => {
    const fake = fakeFiles({});
    const md = `${FAKE_ROOT}/memory/plan.md`;
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
                    path: `${FAKE_ROOT}/b`,
                    kind: "file" as const,
                    size: 1,
                },
                {
                    name: "a",
                    path: `${FAKE_ROOT}/a`,
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
    assert.deepEqual(await resultTexts(agent), [
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
                        message: `Line 1 is 58.6KB, exceeds the 50.0KB limit; showing its first 50.0KB. Use bash: sed -n '1p' ${FAKE_ROOT}/4 | tail -c +51201`,
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
            const at = Number(path.slice(`${FAKE_ROOT}/`.length));
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
                    { path: `${FAKE_ROOT}/${at}`, ...read.args },
                    `o${at}`,
                ),
            ]),
            fauxAssistantMessage("Read."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Read."));
    await agent.idle();

    const taken = await resultTexts(agent);
    assert.equal(taken.length, reads.length * 2);
    for (const [at] of reads.entries()) {
        assert.equal(taken[at * 2 + 1], taken[at * 2], `read ${at}`);
    }
    await agent.stop();
});

test("an own write and an own edit answer in their words, a write that makes its file says so, and the edit gives the workspace's diff", async (t) => {
    const { calls } = fakeCalls({});
    const path = `${FAKE_ROOT}/memory/plan.md`;
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
        // The first write makes the file, as a workspace write that makes one says.
        { ...wrote, details: { created: true } },
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

test("an own write or edit of the root's CHARACTER.md is refused in its words and changes nothing", async (t) => {
    const { calls } = fakeCalls({});
    const files: Record<string, string> = {
        [`${FAKE_ROOT}/CHARACTER.md`]: "Kind.",
    };
    const agent = await wakeAgent(t, {
        calls,
        files: fakeFiles(files),
        responses: [
            toolCall(
                "write",
                { path: "CHARACTER.md", content: "Cruel." },
                "c1",
            ),
            toolCall(
                "edit",
                {
                    path: `${FAKE_ROOT}/CHARACTER.md`,
                    edits: [{ oldText: "Kind.", newText: "Cruel." }],
                },
                "c2",
            ),
            fauxAssistantMessage("Tried."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Write."));
    await agent.idle();

    const refusal = {
        output: "CHARACTER.md is your profile's character, which only a chief changes. Keep what you learn in memory/ instead.",
        isError: true,
        diagnostics: [],
    };
    assert.deepEqual(await toolResults(agent), [refusal, refusal]);
    assert.equal(files[`${FAKE_ROOT}/CHARACTER.md`], "Kind.");
    await agent.stop();
});

test("an own write and edit name their path without `.` parts or doubled slashes", async (t) => {
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

    const path = `${FAKE_ROOT}/memory/plan.md`;
    const [wrote, edited] = await toolResults(agent);
    assert.deepEqual(wrote, {
        output: `Successfully wrote to ${path}`,
        isError: false,
        diagnostics: [],
        details: { created: true },
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

test("a path is own when it normalises into the agent's root", async (t) => {
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
                { path: `/tmp/..${FAKE_ROOT}/memory/x`, content: "in" },
                "c1",
            ),
            toolCall(
                "write",
                { path: `${FAKE_ROOT}/a/../../x`, content: "out" },
                "c2",
            ),
            fauxAssistantMessage("Written."),
        ],
    });
    await agent.actions.submit(agent.by(from.realm), line("input-1", "Write."));
    await agent.idle();

    const [inside, out] = await toolResults(agent);
    assert.equal(inside?.output, `Successfully wrote to ${FAKE_ROOT}/memory/x`);
    assert.deepEqual(own, { [`${FAKE_ROOT}/memory/x`]: "in" });
    assert.equal(out?.output, `Successfully wrote to ${outside}`);
    assert.deepEqual(
        named(sent, "write_file").map(
            (write) => (write.message as { path: string }).path,
        ),
        [outside],
    );
    await agent.stop();
});
