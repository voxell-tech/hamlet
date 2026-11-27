import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage } from "@earendil-works/pi-ai";
import { remember } from "../src/agent/credentials.ts";
import type { Exit } from "../src/agent/messages/workspace.ts";
import {
    FAKE_ROOT,
    fakeCalls,
    fakeFiles,
    from,
    interrupted,
    line,
    named,
    resultTexts,
    timedOut,
    toolCall,
    toolResults,
    until,
    wakeAgent,
} from "./agent-testing.ts";
import { context } from "./testing.ts";

test("bash refuses a timeout of 0 or below in its words before it calls anything", async (t) => {
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
    assert.deepEqual(await resultTexts(agent), [
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

test("a change whose call fails, a deadline included, says it was interrupted, and is not sent again", async (t) => {
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

test("an own edit that a cut-off interrupts is not run again, and says it was interrupted", async (t) => {
    const path = `${FAKE_ROOT}/memory/plan.md`;
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
                confinement: { type: "sandbox", name: "bwrap" },
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

test("a command that ends inside its call answers as bash does", async (t) => {
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
        {
            output: "hi\n",
            isError: false,
            diagnostics: [],
            details: { exitCode: 0 },
        },
        {
            output: "tail\n",
            isError: true,
            details: { exitCode: 2 },
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
            details: { exitCode: -1 },
            diagnostics: [],
        },
        {
            output: "\n[timed out after 120s]",
            isError: true,
            details: { exitCode: -1 },
            diagnostics: [],
        },
        {
            output: "Error: spawn failed. Inspect the output before trying again, or tell the chief.",
            isError: true,
            details: { exitCode: -1 },
            diagnostics: [],
        },
        interrupted("bash"),
        {
            output: "x\n\n[Command 7: cancelled]",
            isError: true,
            diagnostics: [],
        },
        {
            output: "touch: cannot touch '/x': Permission denied\n\n[The sandbox lets a command write only inside the workspace, which is its working directory. Use a relative path there, not /tmp or another folder.]",
            isError: true,
            details: { exitCode: 1 },
            diagnostics: [
                {
                    severity: "error",
                    code: "exit_code",
                    message: "Command exited with code 1",
                },
            ],
        },
        {
            output: "Permission denied\n",
            isError: false,
            diagnostics: [],
            details: { exitCode: 0 },
        },
    ]);
    await agent.stop();
});

test("a hand tool that ends inside its call answers as hand tools do", async (t) => {
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
                confinement: { type: "sandbox", name: "bwrap" },
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
        {
            output: "3 words\n",
            isError: false,
            diagnostics: [],
            details: { exitCode: 0 },
        },
        {
            output: "no file\n",
            isError: true,
            details: { exitCode: 1 },
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
        {
            output: "tail\n",
            isError: false,
            diagnostics: [],
            details: { exitCode: 0 },
        },
    ]);
    await agent.stop();
});

test("a workspace write and edit answer in their words, in the call and later", async (t) => {
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
            details: { created: true },
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

// Every tool's result is scrubbed (C32).
test("an own file tool's result holds no secret the host knows", async (t) => {
    const key = "own-file-key-4f1c9e";
    remember(key);
    const files = fakeFiles({ [`${FAKE_ROOT}/notes.env`]: `TOKEN=${key}\n` });
    const agent = await wakeAgent(t, {
        calls: fakeCalls({}).calls,
        files,
        responses: [
            toolCall("read", { path: `${FAKE_ROOT}/notes.env` }),
            toolCall("edit", {
                path: `${FAKE_ROOT}/notes.env`,
                edits: [{ oldText: "TOKEN", newText: "KEY" }],
            }),
            fauxAssistantMessage("Done."),
        ],
    });
    t.after(() => agent.stop());
    await agent.actions.submit(agent.by(from.realm), line("in-1", "Look"));
    await agent.idle();

    const kept = await resultTexts(agent);
    assert.equal(kept.length, 2);
    assert.ok(kept[0]?.includes("TOKEN=[REDACTED]"), kept[0]);
    assert.ok(
        !JSON.stringify(await agent.entries()).includes(key),
        "no entry holds the key",
    );
});
