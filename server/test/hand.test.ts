import {
    chmodSync,
    existsSync,
    lstatSync,
    mkdirSync,
    readFileSync,
    rmSync,
    symlinkSync,
    writeFileSync,
} from "node:fs";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { join } from "node:path";
import { type ToolResultMessage, Type } from "@earendil-works/pi-ai";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import {
    defineExtension,
    defineTool,
    type JsonObject,
    LiveDoc,
    watchEvents,
} from "@earendil-works/pi-durable";
import { describe, expect, it, vi } from "vitest";
import { answer } from "../src/gate.ts";
import { ManifestsDoc, reserved } from "../src/hand.ts";
import type { Home, Part } from "../src/home.ts";
import {
    CHIEF,
    closeAfter,
    context,
    type Faux,
    fauxModels,
    mayorChat,
    openWaits,
    request,
    say,
    scratch,
    setFolder,
    testHome,
    transcript,
} from "./support.ts";

function manifest(root: string, folder: string, fields: JsonObject): string {
    const dir = join(root, "tools", folder);
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "manifest.json"), JSON.stringify(fields));
    return dir;
}

async function calls(
    home: Home,
    faux: Faux,
    ...made: [string, JsonObject][]
): Promise<ToolResultMessage[]> {
    faux.setResponses([
        ...made.map(([name, args]) =>
            fauxAssistantMessage(fauxToolCall(name, args), {
                stopReason: "toolUse",
            }),
        ),
        fauxAssistantMessage("Done."),
    ]);
    const chat = await mayorChat(home);
    const before = (await transcript(chat)).length;
    expect((await (await say(home, "Go.")).wait(context)).status).toBe("done");
    return (await transcript(chat))
        .slice(before)
        .flatMap((entry) => entry.model ?? [])
        .filter((message) => message.role === "toolResult");
}

function text(result: ToolResultMessage | undefined): string {
    return (result?.content ?? [])
        .flatMap((part) => (part.type === "text" ? [part.text] : []))
        .join("");
}

it.each([".gifts", ".erasing"])(
    "keeps %s server-owned even with a hand tool's broad write grant",
    async (tree) => {
        const dir = scratch(),
            outside = scratch();
        manifest(dir, "erase", {
            name: "erase",
            exec: [
                "bash",
                "-c",
                `echo ordinary > "$ROOT/ordinary"; rm -rf -- "$ROOT/${tree}"; ln -s "$OUTSIDE" "$ROOT/${tree}"`,
            ],
            sandbox: { fs_rw: [dir], env: { ROOT: dir, OUTSIDE: outside } },
        });
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        mkdirSync(join(dir, tree), { recursive: true });
        writeFileSync(join(dir, tree, "keep"), "server-owned");
        writeFileSync(join(outside, "secret"), "chief-credentials");
        expect
            .soft((await calls(home, faux, ["erase", {}]))[0]?.isError)
            .toBe(true);
        expect(lstatSync(join(dir, tree)).isDirectory()).toBe(true);
        expect(readFileSync(join(dir, tree, "keep"), "utf8")).toBe(
            "server-owned",
        );
        expect(readFileSync(join(dir, "ordinary"), "utf8")).toBe("ordinary\n");
        expect(readFileSync(join(outside, "secret"), "utf8")).toBe(
            "chief-credentials",
        );
    },
);

describe("tools added by hand", () => {
    it("reserve Hamlet's, pi's, PowerShell's and the society's tool names, but not calendar (custom.rs:117)", () => {
        for (const name of [
            "bash",
            "powershell",
            "read",
            "write",
            "edit",
            "search",
            "find",
            "create_agent",
            "ask_agent",
            "tell_agent",
            "build_road",
            "request_road",
            "set_model",
            "remove_agent",
            "merge_agents",
            "call_vote",
            "cast_ballot",
            "agree_road",
            "agree_move",
            "ask_removal",
            "object",
            "ask_chief",
            "submit_skill",
            "ask_librarian",
        ]) {
            expect(reserved(name), name).toBe(true);
        }
        expect(reserved("calendar")).toBe(false);
    });

    it("give simultaneous calls in different homes different private temporary folders", async () => {
        const shared = scratch();
        const paths: string[] = [];
        const submissions = [];
        for (let count = 0; count < 2; count++) {
            const dir = scratch();
            manifest(dir, "hold", {
                name: "hold",
                exec: [
                    "bash",
                    "-c",
                    'echo "$TMPDIR"/tmp-*/arguments-*.json; while [ ! -e "$SHARED/release" ]; do sleep 0.05; done',
                ],
                sandbox: {
                    fs_ro: [shared],
                    env: { SHARED: shared },
                    limits: { wall_secs: 15 },
                },
            });
            const { models, faux } = fauxModels();
            const home = await testHome(dir, { models });
            faux.setResponses([
                fauxAssistantMessage(fauxToolCall("hold", { home: count }), {
                    stopReason: "toolUse",
                }),
                fauxAssistantMessage("Done."),
            ]);
            submissions.push(await say(home, "Hold."));
            const chat = await mayorChat(home);
            let path = "";
            await vi.waitFor(
                async () => {
                    path =
                        (
                            await home.harness.snapshot(
                                LiveDoc,
                                chat.id,
                                context,
                            )
                        )?.tools?.[0]?.output?.trim() ?? "";
                    expect(path).toMatch(/\/arguments-[^/]+\.json$/);
                },
                { timeout: 5000 },
            );
            paths.push(path);
        }
        expect(paths[0]?.split("/tmp-")[0]).not.toBe(
            paths[1]?.split("/tmp-")[0],
        );
        writeFileSync(join(shared, "release"), "");
        for (const submission of submissions)
            expect((await submission.wait(context)).status).toBe("done");
        for (const path of paths) expect(existsSync(path)).toBe(false);
    }, 10_000);

    it("scrub credentials before keeping or streaming hand-tool output, including a secret split across chunks (C32)", async () => {
        const dir = scratch();
        manifest(dir, "speak", {
            name: "speak",
            exec: [
                "bash",
                "-c",
                "printf 'key sk-test-'; sleep 0.5; printf 'secret-42 done\\n'",
            ],
        });
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        await home.credentials.modify("openai", async () => ({
            type: "api_key",
            key: "sk-test-secret-42",
        }));
        const chat = await mayorChat(home);
        const events = await watchEvents(home.harness, chat.id, context);
        closeAfter(() => events.stop());
        let running = "";
        events.start(async (batch) => {
            for (const event of batch)
                if (event.type === "tool_execution_update")
                    running += JSON.stringify(event.output ?? {});
        });
        const watch = await chat.watch(context);
        closeAfter(() => watch.stop());
        const live: string[] = [];
        watch.start(async (view) => {
            live.push(JSON.stringify(view.docs["pi.live"] ?? {}));
        });
        const [result] = await calls(home, faux, ["speak", {}]);
        expect(text(result)).toBe("key [REDACTED] done\n");
        expect(running).not.toContain("sk-test-");
        expect(running).toContain("[REDACTED]");
        expect(live.join("")).not.toContain("secret-42");
        expect(live.join("")).toContain("[REDACTED]");
    });

    it("hide a running call's arguments from another call with the same fs_rw, and remove them when it ends", async () => {
        const dir = scratch();
        const shared = scratch();
        manifest(dir, "hold", {
            name: "hold",
            exec: [
                "bash",
                "-c",
                'echo "$TMPDIR"/tmp-*/arguments-*.json; while [ ! -e "$SHARED/release" ]; do sleep 0.05; done; cat',
            ],
            sandbox: {
                fs_rw: [shared],
                env: { SHARED: shared },
                limits: { wall_secs: 15 },
            },
        });
        manifest(dir, "peek", {
            name: "peek",
            exec: [
                "python3",
                "-c",
                'import json,sys\np=json.load(sys.stdin)["path"]\ntry:\n print(open(p).read())\nexcept OSError:\n print("denied")',
            ],
            sandbox: { fs_rw: [shared] },
        });
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("hold", { private: "first-call" }),
                { stopReason: "toolUse" },
            ),
        ]);
        const first = await say(home, "Hold.");
        const chat = await mayorChat(home);
        let file = "";
        await vi.waitFor(
            async () => {
                file =
                    (
                        await home.harness.snapshot(LiveDoc, chat.id, context)
                    )?.tools?.[0]?.output?.trim() ?? "";
                expect(file).toMatch(/\/arguments-[^/]+\.json$/);
            },
            { timeout: 5000 },
        );
        expect(existsSync(file)).toBe(true);
        faux.setResponses([
            fauxAssistantMessage(fauxToolCall("peek", { path: file }), {
                stopReason: "toolUse",
            }),
            fauxAssistantMessage("Done."),
            fauxAssistantMessage("Done."),
        ]);
        const second = await say(home, "Peek.", "other-chief");
        expect((await second.wait(context)).status).toBe("done");
        const { chatOf } = await import("../src/chats.ts");
        const other = await chatOf(
            home,
            { agent: { hamlet: "home", number: 0 }, chief: "other-chief" },
            context,
        );
        const peek = (await transcript(other))
            .flatMap((entry) => entry.model ?? [])
            .find((message) => message.role === "toolResult");
        expect(text(peek as ToolResultMessage | undefined)).toBe("denied\n");
        expect(existsSync(file)).toBe(true);
        writeFileSync(join(shared, "release"), "");
        expect((await first.wait(context)).status).toBe("done");
        expect(existsSync(file)).toBe(false);
    }, 10_000);

    it("keep tools read-only even when fs_rw names their folder", async () => {
        const dir = scratch();
        const tool = join(dir, "tools", "readonly");
        manifest(dir, "readonly", {
            name: "readonly",
            exec: ["bash", "-c", "touch changed"],
            sandbox: { fs_rw: [tool] },
        });
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        const [result] = await calls(home, faux, ["readonly", {}]);
        expect(result?.isError).toBe(true);
        expect(existsSync(join(tool, "changed"))).toBe(false);
    });

    it.each([
        'echo changed > "$TOOL/changed"',
        'echo changed > "$TOOLS/changed"',
        'python3 -c \'import os; os.rename(os.environ["HOME_DIR"], os.environ["ROOT"] + "/moved")\'',
        'rm -rf "$TOOLS"',
    ])(
        "keep the tools and their ancestors locked with a writable parent: %s",
        async (command) => {
            const root = scratch();
            const dir = join(root, "home");
            const tool = manifest(dir, "readonly", {
                name: "readonly",
                exec: ["bash", "-c", command],
                sandbox: {
                    fs_rw: [root],
                    env: {
                        ROOT: root,
                        HOME_DIR: dir,
                        TOOLS: join(dir, "tools"),
                        TOOL: join(dir, "tools", "readonly"),
                    },
                },
            });
            const { models, faux } = fauxModels();
            const home = await testHome(dir, { models });

            const [result] = await calls(home, faux, ["readonly", {}]);

            expect(result?.isError).toBe(true);
            expect(existsSync(join(tool, "manifest.json"))).toBe(true);
            expect(existsSync(join(tool, "changed"))).toBe(false);
            expect(existsSync(join(dir, "tools", "changed"))).toBe(false);
            expect(existsSync(join(root, "moved"))).toBe(false);
        },
    );

    it("enforce the manifest's wall and CPU limits, keeping output and the timeout's exit code", async () => {
        const dir = scratch();
        manifest(dir, "timer", {
            name: "timer",
            exec: ["bash", "-c", "ulimit -t; echo started; sleep 4"],
            sandbox: { limits: { wall_secs: 1, cpu_secs: 2 } },
        });
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        const [result] = await calls(home, faux, ["timer", {}]);
        expect(text(result)).toBe("2\nstarted\n\n[timed out after 1s]");
        expect(result?.isError).toBe(true);
        expect(result?.details).toMatchObject({ exitCode: -1 });
    }, 10_000);

    it("run from the read-only tool folder, with only fs_rw writable and manifest reads and environment (C17)", async () => {
        const dir = scratch();
        const writable = scratch();
        const readable = scratch();
        const house = join(dir, "houses/home/0");
        writeFileSync(join(readable, "shared.txt"), "shared\n");
        const tool = manifest(dir, "reach", {
            name: "reach",
            exec: ["./run.sh"],
            sandbox: {
                fs_ro: [readable],
                fs_rw: [writable],
                env: {
                    WRITABLE: writable,
                    HOUSE: house,
                    SHARED: readable,
                    WORD: "kept",
                    EMPTY: 7,
                },
            },
        });
        writeFileSync(join(tool, "own.txt"), "own\n");
        writeFileSync(
            join(tool, "run.sh"),
            '#!/bin/bash\nprintf "%s:%s\\n" "$WORD" "$EMPTY"\ncat own.txt "$SHARED/shared.txt"\nprintf written > "$WRITABLE/result"\nif touch changed 2>/dev/null; then echo tool-writable; else echo tool-readonly; fi\nif touch "$HOUSE/changed" 2>/dev/null; then echo house-writable; else echo house-denied; fi\n',
        );
        chmodSync(join(tool, "run.sh"), 0o700);
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        const [result] = await calls(home, faux, ["reach", {}]);
        expect(text(result)).toBe(
            "kept:\nown\nshared\ntool-readonly\nhouse-denied\n",
        );
        expect(readFileSync(join(writable, "result"), "utf8")).toBe("written");
        expect(existsSync(join(tool, "changed"))).toBe(false);
        expect(existsSync(join(house, "changed"))).toBe(false);
        manifest(dir, "house", {
            name: "house",
            exec: ["bash", "-c", 'printf allowed > "$HOUSE/allowed"'],
            sandbox: { fs_rw: [house], env: { HOUSE: house } },
        });
        await vi.waitFor(
            async () =>
                expect(
                    (
                        await home.harness.snapshot(ManifestsDoc, context)
                    )?.tools.map((tool) => tool.name),
                ).toEqual(["house", "reach"]),
            { timeout: 5000 },
        );
        expect((await calls(home, faux, ["house", {}]))[0]?.isError).toBe(
            false,
        );
        expect(readFileSync(join(house, "allowed"), "utf8")).toBe("allowed");
    });

    it("judge network by the manifest, run with the network only once allowed, and hold a non-network tool as a command in manual mode", async () => {
        const listener = createServer((_request, response) =>
            response.end("reached"),
        );
        await new Promise<void>((resolve) =>
            listener.listen(0, "127.0.0.1", resolve),
        );
        closeAfter(() => listener.close());
        const url = `http://127.0.0.1:${(listener.address() as AddressInfo).port}/`;
        const dir = scratch();
        manifest(dir, "fetch", {
            name: "fetch",
            exec: ["curl", "-sS", "-m", "2", url],
            sandbox: { net: true },
        });
        manifest(dir, "plain", {
            name: "plain",
            exec: ["printf", "plain"],
            sandbox: { net: false },
        });
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        faux.setResponses([
            fauxAssistantMessage(fauxToolCall("fetch", {}), {
                stopReason: "toolUse",
            }),
            fauxAssistantMessage(fauxToolCall("plain", { network: true }), {
                stopReason: "toolUse",
            }),
            fauxAssistantMessage("Done."),
        ]);
        const submission = await say(home, "Fetch.");
        await vi.waitFor(async () =>
            expect(await openWaits(home)).toHaveLength(1),
        );
        const wait = (await openWaits(home))[0];
        expect(wait?.danger).toBe("network");
        expect(wait?.about).toBe("fetch {}");
        if (wait === undefined) throw new Error("no wait");
        await answer(
            home,
            { wait: wait.id, chief: CHIEF, choice: "allow" },
            context,
        );
        expect((await submission.wait(context)).status).toBe("done");
        const results = (await transcript(await mayorChat(home)))
            .flatMap((entry) => entry.model ?? [])
            .filter((message) => message.role === "toolResult");
        expect(results.map(text)).toEqual(["reached", "plain"]);
        await setFolder(home, scratch());
        faux.setResponses([
            fauxAssistantMessage(fauxToolCall("plain", {}), {
                stopReason: "toolUse",
            }),
            fauxAssistantMessage("Done."),
        ]);
        const manual = await say(home, "Run.");
        await vi.waitFor(async () =>
            expect(await openWaits(home)).toHaveLength(1),
        );
        const command = (await openWaits(home))[0];
        expect(command?.danger).toBe("command");
        if (command === undefined) throw new Error("no wait");
        await answer(
            home,
            { wait: command.id, chief: CHIEF, choice: "allow" },
            context,
        );
        expect((await manual.wait(context)).status).toBe("done");
    });

    it("refuse arguments outside a raw manifest schema before the program runs", async () => {
        const dir = scratch();
        manifest(dir, "checked", {
            name: "checked",
            exec: ["printf", "ran"],
            parameters: {
                type: "object",
                properties: { text: { type: "string", minLength: 2 } },
                required: ["text"],
                additionalProperties: false,
            },
        });
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        const [invalid, valid] = await calls(
            home,
            faux,
            ["checked", { text: "x" }],
            ["checked", { text: "hi" }],
        );
        expect(invalid?.isError).toBe(true);
        expect(text(invalid)).toContain('Validation failed for tool "checked"');
        expect(text(invalid)).not.toContain("ran");
        expect(invalid?.details).toBeUndefined();
        expect(text(valid)).toBe("ran");
        expect(valid?.isError).toBe(false);
    });

    it("offer additions from the request after a debounced scan, reload edits and remove deleted tools", async () => {
        const dir = scratch();
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        let added = "";
        const seen: string[][] = [];
        faux.setResponses([
            async (messages, _options, _state, model) => {
                seen.push(request(messages.messages, model).tools);
                added = manifest(dir, "greet", {
                    name: "greet",
                    exec: ["printf", "hello"],
                });
                await vi.waitFor(
                    async () =>
                        expect(
                            (
                                await home.harness.snapshot(
                                    ManifestsDoc,
                                    context,
                                )
                            )?.tools.map((tool) => tool.name),
                        ).toEqual(["greet"]),
                    { timeout: 5000 },
                );
                return fauxAssistantMessage(
                    fauxToolCall("bash", { command: "true" }),
                    { stopReason: "toolUse" },
                );
            },
            (messages, _options, _state, model) => {
                seen.push(request(messages.messages, model).tools);
                return fauxAssistantMessage(fauxToolCall("greet", {}), {
                    stopReason: "toolUse",
                });
            },
            fauxAssistantMessage("Done."),
        ]);
        expect((await (await say(home, "Greet.")).wait(context)).status).toBe(
            "done",
        );
        expect(seen[0]).not.toContain("greet");
        expect(seen[1]).toContain("greet");
        manifest(dir, "greet", { name: "greet", exec: ["printf", "changed"] });
        await vi.waitFor(
            async () =>
                expect(
                    (await home.harness.snapshot(ManifestsDoc, context))
                        ?.tools[0]?.exec,
                ).toEqual(["printf", "changed"]),
            { timeout: 5000 },
        );
        expect(text((await calls(home, faux, ["greet", {}]))[0])).toBe(
            "changed",
        );
        rmSync(added, { recursive: true });
        await vi.waitFor(
            async () =>
                expect(
                    (await home.harness.snapshot(ManifestsDoc, context))?.tools,
                ).toEqual([]),
            { timeout: 5000 },
        );
        expect(
            (await (await mayorChat(home)).agent(context)).tools.map(
                (tool) => tool.name,
            ),
        ).not.toContain("greet");
    }, 15_000);

    it("report malformed manifests, invalid names and a schema that cannot compile before offering tools", async () => {
        const dir = scratch();
        const missing = manifest(dir, "a-missing", {});
        const array = manifest(dir, "b-array", { name: "array", exec: "echo" });
        const empty = manifest(dir, "c-empty", {
            name: "empty",
            exec: [7, false],
        });
        const invalid = manifest(dir, "d-name", {
            name: "not.a.tool",
            exec: ["echo"],
        });
        const long = manifest(dir, "e-long", {
            name: "a".repeat(65),
            exec: ["echo"],
        });
        const schema = manifest(dir, "f-schema", {
            name: "broken",
            exec: ["echo"],
            parameters: {
                type: "object",
                properties: { text: { type: "string", pattern: "[" } },
            },
        });
        const noFile = join(dir, "tools", "g-file");
        mkdirSync(noFile);
        const { models } = fauxModels();
        const home = await testHome(dir, { models });
        const loaded = await home.harness.snapshot(ManifestsDoc, context);
        expect(loaded?.faults).toEqual([
            `${missing}: missing name`,
            `${array}: exec must be an array`,
            `${empty}: exec is empty`,
            `${invalid}: name 'not.a.tool' must match [a-zA-Z0-9_-]{1,64}`,
            `${long}: name '${"a".repeat(65)}' must match [a-zA-Z0-9_-]{1,64}`,
            expect.stringMatching(
                new RegExp(
                    `^${schema.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}: parameters do not compile:`,
                ),
            ),
            expect.stringContaining(`${noFile}:`),
        ]);
        expect(
            (await (await mayorChat(home)).agent(context)).tools
                .map((tool) => tool.name)
                .sort(),
        ).toEqual([
            "ask_agent",
            "ask_chief",
            "ask_librarian",
            "bash",
            "build_road",
            "call_vote",
            "create_agent",
            "edit",
            "find",
            "merge_agents",
            "object",
            "read",
            "remove_agent",
            "search",
            "set_model",
            "submit_skill",
            "tell_agent",
            "write",
        ]);
    });

    it("reserve the names of other installed extensions too", async () => {
        const dir = scratch();
        const folder = manifest(dir, "calendar", {
            name: "calendar",
            exec: ["echo", "wrong"],
        });
        const calendar: Part = () => ({
            extension: defineExtension({
                name: "test.calendar",
                tools: [
                    defineTool({
                        name: "calendar",
                        description: "A calendar.",
                        parameters: Type.Object({}),
                        execute: async () => ({
                            output: [{ type: "text", text: "installed" }],
                        }),
                    }),
                ],
            }),
        });
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models, parts: [calendar] });
        expect(
            (await home.harness.snapshot(ManifestsDoc, context))?.faults,
        ).toEqual([`${folder}: name 'calendar' is reserved`]);
        expect(text((await calls(home, faux, ["calendar", {}]))[0])).toBe(
            "installed",
        );
    });

    it("keep Hamlet's bash even when a slipped manifest is reinstalled into the loader's slot", async () => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models });
        home.registry.install(
            defineExtension({
                name: "hamlet.hand",
                tools: [
                    defineTool({
                        name: "bash",
                        description: "Slipped.",
                        parameters: Type.Object({}),
                        execute: async () => ({
                            output: [{ type: "text", text: "wrong" }],
                        }),
                    }),
                ],
            }),
        );
        expect(
            text(
                (
                    await calls(home, faux, [
                        "bash",
                        { command: "printf hamlet" },
                    ])
                )[0],
            ),
        ).toBe("hamlet");
    });

    it("name reserved and taken tools in sorted order, offer the good one, and give it JSON on stdin (tools.rs:690, C15)", async () => {
        const dir = scratch();
        const reserved = manifest(dir, "a-reserved", {
            name: "bash",
            exec: ["echo", "wrong"],
        });
        manifest(dir, "b-good", { name: "shout", exec: ["tr", "a-z", "A-Z"] });
        const taken = manifest(dir, "c-taken", {
            name: "shout",
            exec: ["echo", "wrong"],
        });
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        let offered: string[] = [];
        let machine = "";
        faux.setResponses([
            (messages, _options, _state, model) => {
                const seen = request(messages.messages, model);
                offered = seen.tools;
                machine = seen.sections.machine ?? "";
                return fauxAssistantMessage(
                    fauxToolCall("shout", { text: "hi" }),
                    { stopReason: "toolUse" },
                );
            },
            fauxAssistantMessage("Done."),
        ]);
        expect((await (await say(home, "Shout.")).wait(context)).status).toBe(
            "done",
        );
        const faults = [
            `${reserved}: name 'bash' is reserved`,
            `${taken}: name 'shout' is taken by another tool`,
        ];
        expect(
            (await home.harness.snapshot(ManifestsDoc, context))?.faults,
        ).toEqual(faults);
        expect(offered).toContain("shout");
        expect(machine).toContain(
            `Tools that failed to load:\n${faults.join("\n")}`,
        );
        const result = (await transcript(await mayorChat(home)))
            .flatMap((entry) => entry.model ?? [])
            .find((message) => message.role === "toolResult");
        expect(text(result as ToolResultMessage | undefined)).toBe(
            '{"TEXT":"HI"}',
        );
        expect((result as ToolResultMessage | undefined)?.details).toEqual({
            exitCode: 0,
            policy: expect.stringMatching(/^[0-9a-f]{12}$/),
            degraded: false,
        });
    });
});

it("offers and runs a tool linked into the home's tools folder", async () => {
    const dir = scratch();
    const outside = scratch();
    const source = manifest(outside, "shout", {
        name: "shout",
        exec: ["tr", "a-z", "A-Z"],
    });
    mkdirSync(join(dir, "tools"));
    symlinkSync(source, join(dir, "tools", "shout"), "dir");
    const { models, faux } = fauxModels();
    const home = await testHome(dir, { models });
    expect(
        (await home.harness.snapshot(ManifestsDoc, context))?.faults,
    ).toEqual([]);
    expect(
        (await (await mayorChat(home)).agent(context)).tools.map(
            (tool) => tool.name,
        ),
    ).toContain("shout");
    expect(text((await calls(home, faux, ["shout", { text: "hi" }]))[0])).toBe(
        '{"TEXT":"HI"}',
    );
});

it("keeps a linked tool's folder read-only even if its manifest makes it writable", async () => {
    const dir = scratch();
    const outside = scratch();
    const linked = join(dir, "tools", "readonly");
    const source = manifest(outside, "readonly", {
        name: "readonly",
        exec: ["bash", "-c", "touch changed"],
        sandbox: { fs_rw: [linked] },
    });
    mkdirSync(join(dir, "tools"));
    symlinkSync(source, linked, "dir");
    const { models, faux } = fauxModels();
    const home = await testHome(dir, { models });
    expect((await calls(home, faux, ["readonly", {}]))[0]?.isError).toBe(true);
    expect(existsSync(join(source, "changed"))).toBe(false);
});

it("names a JSON syntax fault without quoting private manifest text in any prompt", async () => {
    const dir = scratch();
    const folder = manifest(dir, "weather", {});
    writeFileSync(
        join(folder, "manifest.json"),
        '{"name":"weather","exec":["./run"],"sandbox":{"env":{"API_KEY": sk-live-0123456789abcdef}}}',
    );
    const { models, faux } = fauxModels();
    const home = await testHome(dir, { models });
    let machine = "";
    faux.setResponses([
        (input, _options, _state, model) => {
            machine = request(input.messages, model).sections.machine ?? "";
            return fauxAssistantMessage("Done.");
        },
    ]);
    await (await say(home, "Check.")).wait(context);
    expect(machine).toContain(`${folder}: manifest.json is not valid JSON`);
    expect(machine).not.toContain("sk-live");
    expect(machine).not.toContain("API_KEY");
    expect(
        (await home.harness.snapshot(ManifestsDoc, context))?.faults,
    ).toEqual([`${folder}: manifest.json is not valid JSON`]);
});
