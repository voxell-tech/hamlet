import {
    existsSync,
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
    hook,
    type JsonObject,
    ToolTask,
    watchEvents,
} from "@earendil-works/pi-durable";
import { describe, expect, it } from "vitest";
import { LEAVE } from "../src/environment.ts";
import type { Home } from "../src/home.ts";
import type { Part } from "../src/parts.ts";
import { bashTool } from "../src/tools/shell.ts";
import {
    closeAfter,
    context,
    type Faux,
    fauxModels,
    mayorChat,
    say,
    scratch,
    setFolder,
    testHome,
    transcript,
} from "./support.ts";

/** A home whose mayor runs faux::faux-1, with `parts` added. */
async function mayorHome(parts: Part[] = []) {
    const dir = scratch();
    const { models, faux } = fauxModels();
    const home = await testHome(dir, { models, parts });
    return { dir, home, faux, house: join(dir, "houses", "home", "0") };
}

/** The result of each call the mayor makes for one line, one call to a response, then "Done." */
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
    const before = (await transcript(await mayorChat(home))).length;
    expect((await (await say(home, "Go.")).wait(context)).status).toBe("done");
    return (await transcript(await mayorChat(home)))
        .slice(before)
        .flatMap((entry) => entry.model ?? [])
        .filter((message) => message.role === "toolResult");
}

/** The text of a tool's result. */
function text(result: ToolResultMessage | undefined): string {
    return (result?.content ?? [])
        .flatMap((part) => (part.type === "text" ? [part.text] : []))
        .join("");
}

const OUTSIDE =
    "\n[The sandbox lets a command write only inside the workspace, which is its working directory. Use a relative path there, not /tmp or another folder.]";
const OUTSIDE_PROJECT =
    "\n[The sandbox lets a command write only in the project folder and in your own workspace, and keeps Hamlet's own folder closed to it.]";

describe("the mayor's tools", () => {
    it("are read, write, edit, bash, search and find, with or without a folder", async () => {
        const { home } = await mayorHome();
        const offered = async () =>
            (await (await mayorChat(home)).agent(context)).tools
                .map((tool) => tool.name)
                .sort();
        const all = [
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
        ];

        expect(await offered()).toEqual(all);
        await setFolder(home, scratch(), "auto");
        expect(await offered()).toEqual(all);
    });

    it("run bash in the house, keep stderr, and give an exit that is not 0 as an error with its code, policy and sandbox (tools.rs:644)", async () => {
        const { home, faux, house } = await mayorHome();

        const [result] = await calls(home, faux, [
            "bash",
            { command: "pwd -P; echo err >&2; exit 3" },
        ]);

        expect(text(result)).toContain("houses/home/0");
        expect(text(result)).toContain("err");
        expect(result?.isError).toBe(true);
        expect(result?.details).toEqual({
            exitCode: 3,
            policy: expect.stringMatching(/^[0-9a-f]{12}$/),
            degraded: false,
        });
        expect(existsSync(house)).toBe(true);
    });

    it("reach the network with bash for the one call whose leave grants it, and `network` never reaches the shell (C19)", async () => {
        const server = createServer((_request, response) =>
            response.end("reached"),
        );
        await new Promise<void>((resolve) =>
            server.listen(0, "127.0.0.1", resolve),
        );
        closeAfter(() => server.close());
        const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;
        // A gate for this shell proof, granting only a call's explicit flag.
        const leave: Part = () => ({
            extension: defineExtension({
                name: "hamlet.gate",
                hooks: [
                    hook(ToolTask, {
                        beforeTool: async (call, api, context) => {
                            if (call.arguments.network === true) {
                                await api.memo(LEAVE, { net: true }, context);
                            }
                            return undefined;
                        },
                    }),
                ],
            }),
        });
        const { home, faux } = await mayorHome([leave]);

        const [granted, plain, next] = await calls(
            home,
            faux,
            ["bash", { command: `curl -sS -m 2 ${url}`, network: true }],
            ["bash", { command: `curl -sS -m 2 ${url}` }],
            ["bash", { command: `echo "[$network]"; curl -sS -m 2 ${url}` }],
        );

        expect(text(granted)).toBe("reached");
        expect(text(plain)).not.toContain("reached");
        expect(text(next)).toContain("[]");
        expect(text(next)).not.toContain("reached");
    });

    it("stop a command at the default time, and a longer time the model asks for at the same", async () => {
        const short: Part = (home) => ({
            extension: defineExtension({
                name: "test.short",
                tools: [bashTool(home, 1)],
            }),
        });
        const { home, faux } = await mayorHome([short]);

        const [plain, longer] = await calls(
            home,
            faux,
            ["bash", { command: "echo started; sleep 5" }],
            ["bash", { command: "sleep 5", timeout: 30 }],
        );

        expect(text(plain)).toBe("started\n\n[timed out after 1s]");
        expect(plain?.isError).toBe(true);
        expect(plain?.details).toMatchObject({ exitCode: -1 });
        expect(text(longer)).toBe("\n[timed out after 1s]");
    });

    it("hint where a command may write when the sandbox refuses one, with a folder and without (tools.rs:498-507)", async () => {
        const { home, faux } = await mayorHome();
        const outside = { command: "touch /usr/hamlet-outside" };

        const [alone] = await calls(home, faux, ["bash", outside]);
        await setFolder(home, scratch(), "auto");
        const [working] = await calls(home, faux, ["bash", outside]);

        expect(text(alone)).toContain(OUTSIDE);
        expect(text(working)).toContain(OUTSIDE_PROJECT);
    });

    it("fail a command before it runs when the folder is gone", async () => {
        const { home, faux } = await mayorHome();
        const folder = scratch();
        await setFolder(home, folder, "auto");
        rmSync(folder, { recursive: true });

        const [result] = await calls(home, faux, [
            "bash",
            { command: "echo ran" },
        ]);

        expect(text(result)).toBe(
            `Error: the folder ${folder} is gone. Tell the chief.`,
        );
        expect(result?.isError).toBe(true);
    });

    it("put a relative path of read, write or edit in the folder, but NOTES.md and skills/ in the house (edit.rs:320)", async () => {
        const { home, faux, house } = await mayorHome();
        const folder = scratch();
        await setFolder(home, folder, "auto");

        const results = await calls(
            home,
            faux,
            ["write", { path: "plan.txt", content: "plan\n" }],
            [
                "write",
                {
                    path: "skills/new/SKILL.md",
                    content: "---\nname: new\n---\n",
                },
            ],
            [
                "edit",
                {
                    path: "NOTES.md",
                    edits: [{ oldText: "# Notes", newText: "# Notes\nTea." }],
                },
            ],
            ["read", { path: "plan.txt" }],
        );

        expect(results.map((result) => result.isError)).toEqual([
            false,
            false,
            false,
            false,
        ]);
        expect(readFileSync(join(folder, "plan.txt"), "utf8")).toBe("plan\n");
        expect(existsSync(join(house, "skills/new/SKILL.md"))).toBe(true);
        expect(readFileSync(join(house, "NOTES.md"), "utf8")).toBe(
            "# Notes\nTea.\n",
        );
        expect(existsSync(join(folder, "NOTES.md"))).toBe(false);
        expect(existsSync(join(folder, "skills"))).toBe(false);
        expect(text(results[3])).toContain("plan");
    });

    it.each([false, true])(
        "write and edit git files with a folder: %s",
        async (hasFolder) => {
            const { home, faux, house } = await mayorHome();
            const root = hasFolder ? scratch() : house;
            if (hasFolder) await setFolder(home, root, "auto");

            for (const path of [
                ".git/hooks/pre-commit",
                ".git/config",
                "vendor/lib/.git/hooks/pre-commit",
                "vendor/lib/.git/config",
            ]) {
                const results = await calls(
                    home,
                    faux,
                    ["write", { path, content: "before\n" }],
                    [
                        "edit",
                        {
                            path,
                            edits: [{ oldText: "before", newText: "after" }],
                        },
                    ],
                );
                expect(results.map((result) => result.isError)).toEqual([
                    false,
                    false,
                ]);
                expect(readFileSync(join(root, path), "utf8")).toBe("after\n");
            }
        },
    );

    it.each([
        ["no folder", false],
        ["a folder", true],
    ])(
        "write nothing out of reach through .., a full path or a link, with %s (tools.rs:600; edit.rs:301)",
        async (_with, hasFolder) => {
            const { dir, home, faux, house } = await mayorHome();
            const elsewhere = scratch();
            mkdirSync(join(dir, "houses/home/1"), { recursive: true });
            symlinkSync(elsewhere, join(house, "out"));
            if (hasFolder) await setFolder(home, scratch(), "auto");

            const results = await calls(
                home,
                faux,
                [
                    "write",
                    { path: join(dir, "houses/home/1/NOTES.md"), content: "x" },
                ],
                ["write", { path: "../1/NOTES.md", content: "x" }],
                ["write", { path: join(elsewhere, "x"), content: "x" }],
                ["write", { path: join(house, "out/x"), content: "x" }],
                ["write", { path: join(house, "out/sub/x"), content: "x" }],
                [
                    "write",
                    { path: join(dir, "credentials.json"), content: "x" },
                ],
            );

            expect(results.map((result) => result.isError)).toEqual(
                results.map(() => true),
            );
            expect(existsSync(join(dir, "houses/home/1/NOTES.md"))).toBe(false);
            expect(existsSync(join(elsewhere, "x"))).toBe(false);
            expect(existsSync(join(elsewhere, "sub"))).toBe(false);
            expect(
                readFileSync(join(dir, "credentials.json"), "utf8"),
            ).not.toBe("x");
        },
    );

    it("leave a file that is not UTF-8 byte for byte as it was (edit.rs:272)", async () => {
        const { home, faux, house } = await mayorHome();
        const bytes = Buffer.from([0x61, 0xff, 0xfe, 0x62, 0x0a]);
        writeFileSync(join(house, "raw.txt"), bytes);

        const [result] = await calls(home, faux, [
            "edit",
            { path: "raw.txt", edits: [{ oldText: "a", newText: "A" }] },
        ]);

        expect(result?.isError).toBe(true);
        expect(readFileSync(join(house, "raw.txt"))).toEqual(bytes);
    });

    it.each([
        ["read", { path: ".env" }],
        ["search", { pattern: "KEY|TOKEN", path: "." }],
        ["find", { path: "." }],
    ] as const)(
        "scrub credentials from %s results before the model or kept conversation reads them (C32, S7)",
        async (name, args) => {
            const { home, faux } = await mayorHome();
            const folder = scratch();
            const secret = "sk-review-secret-77";
            await home.credentials.modify("openai", async () => ({
                type: "api_key",
                key: secret,
            }));
            writeFileSync(
                join(folder, ".env"),
                `KEY=${secret}\nTOKEN=${home.credentials.token}\n`,
            );
            writeFileSync(join(folder, secret), "");
            writeFileSync(join(folder, home.credentials.token), "");
            await setFolder(home, folder, "auto");

            const [result] = await calls(home, faux, [name, args]);

            expect(result?.isError).toBe(false);
            expect(text(result)).toContain("[REDACTED]");
            expect(text(result)).not.toContain(secret);
            expect(text(result)).not.toContain(home.credentials.token);
        },
    );

    it("scrubs structured results before a nested caller reads them and keeps result details redacted (C32, S7)", async () => {
        let nested: unknown;
        const tools: Part = (home) => ({
            extension: defineExtension({
                name: "test.structured",
                tools: [
                    defineTool({
                        name: "credentials",
                        description: "Returns credentials for the scrub proof.",
                        parameters: Type.Object({}),
                        structuredOutputSchema: Type.Object({
                            values: Type.Array(Type.String()),
                        }),
                        execute: async () => ({
                            output: [
                                {
                                    type: "text",
                                    text: "key sk-review-secret-77",
                                },
                            ],
                            structuredOutput: {
                                values: [
                                    "sk-review-secret-77",
                                    home.credentials.token,
                                ],
                            },
                            details: { token: home.credentials.token },
                        }),
                    }),
                    defineTool({
                        name: "collect",
                        description: "Reads a nested tool's public result.",
                        parameters: Type.Object({}),
                        execute: async (_args, api, context) => {
                            const result = await api.executeTool(
                                "credentials",
                                {},
                                context,
                                { progress: false },
                            );
                            nested = result.structuredOutput;
                            return {
                                output: [{ type: "text", text: "Done." }],
                                details: result.details ?? null,
                            };
                        },
                    }),
                ],
            }),
        });
        const { home, faux } = await mayorHome([tools]);
        await home.credentials.modify("openai", async () => ({
            type: "api_key",
            key: "sk-review-secret-77",
        }));

        const [result] = await calls(home, faux, ["collect", {}]);

        expect(result?.isError).toBe(false);
        expect(nested).toEqual({ values: ["[REDACTED]", "[REDACTED]"] });
        expect(result?.details).toEqual({ token: "[REDACTED]" });
    });

    it("keep complete spilled output readable without credentials, including secrets split across chunks (C32)", async () => {
        const { home, faux } = await mayorHome();
        await home.credentials.modify("openai", async () => ({
            type: "api_key",
            key: "sk-test-secret-42",
        }));
        let spill = "";
        faux.setResponses([
            fauxAssistantMessage(
                fauxToolCall("bash", {
                    command: `printf 'start\\n'; seq 1 3000; printf 'key sk-test-'; sleep 0.05; printf 'secret-42\\n'; echo ${home.credentials.token}`,
                }),
                { stopReason: "toolUse" },
            ),
            (transcript) => {
                const result = transcript.messages.findLast(
                    (message) => message.role === "toolResult",
                );
                spill =
                    text(result as ToolResultMessage | undefined).match(
                        /Full output: ([^\n]+)/,
                    )?.[1] ?? "";
                return fauxAssistantMessage(
                    fauxToolCall("read", { path: spill, offset: 2999 }),
                    { stopReason: "toolUse" },
                );
            },
            fauxAssistantMessage("Done."),
        ]);
        expect(
            (await (await say(home, "Show the long output.")).wait(context))
                .status,
        ).toBe("done");
        const results = (await transcript(await mayorChat(home)))
            .flatMap((entry) => entry.model ?? [])
            .filter((message) => message.role === "toolResult");

        expect(spill).not.toBe("");
        expect(results.map((result) => result.isError)).toEqual([false, false]);
        expect(text(results[1])).toContain("key [REDACTED]\n[REDACTED]");
        expect(results.map(text).join("")).not.toContain(
            home.credentials.token,
        );
        const full = readFileSync(spill, "utf8");
        expect(full).toContain("start\n1\n2\n");
        expect(full).toContain("key [REDACTED]\n[REDACTED]\n");
        expect(full).not.toContain("sk-test-secret-42");
        expect(full).not.toContain(home.credentials.token);
    });

    it("keep a credential a command prints out of its running output and its result, even split across two chunks (C32)", async () => {
        const { home, faux } = await mayorHome();
        const secret = "sk-test-secret-42";
        await home.credentials.modify("openai", async () => ({
            type: "api_key",
            key: secret,
        }));
        const chat = await mayorChat(home);
        const events = await watchEvents(home.harness, chat.id, context);
        closeAfter(() => events.stop());
        let running = "";
        events.start(async (batch) => {
            for (const event of batch) {
                if (event.type !== "tool_execution_update") continue;
                const { output } = event;
                if (output !== undefined) running += JSON.stringify(output);
            }
        });
        const live: string[] = [];
        const watch = await chat.watch(context);
        closeAfter(() => watch.stop());
        watch.start(async (view) => {
            live.push(JSON.stringify(view.docs["pi.live"] ?? {}));
        });

        const [result] = await calls(home, faux, [
            "bash",
            {
                command: `printf 'key sk-test-'; sleep 0.5; printf 'secret-42 done\\n'; echo ${home.credentials.token}`,
            },
        ]);

        expect(text(result)).toBe("key [REDACTED] done\n[REDACTED]\n");
        expect(running).not.toContain("sk-test-");
        expect(running).toContain("[REDACTED]");
        expect(live.join("")).not.toContain("secret-42");
        expect(live.join("")).not.toContain(home.credentials.token);
    });
});
