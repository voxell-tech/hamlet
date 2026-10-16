import { existsSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
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
} from "@earendil-works/pi-durable";
import { describe, expect, it, vi } from "vitest";
import { resetTrust } from "../src/asked.ts";
import { chatOf, startConversation, WithDoc } from "../src/chats.ts";
import { LEAVE, type Leave } from "../src/environment.ts";
import { answer } from "../src/gate.ts";
import { GrantsDoc, grantsOf, revoke } from "../src/grants.ts";
import { HOME } from "../src/hamlet.ts";
import type { Home, Part } from "../src/home.ts";
import { buildHouse } from "../src/home.ts";
import { LimitsDoc } from "../src/limits.ts";
import { ManifestsDoc } from "../src/manifests.ts";
import { RecordDoc } from "../src/record.ts";
import { SettingsDoc } from "../src/settings.ts";
import {
    type Choice,
    OversightDayDoc,
    raise,
    type Wait,
} from "../src/waits.ts";
import {
    CHIEF,
    closeAfter,
    context,
    crashing,
    type Faux,
    fauxModels,
    mayorChat,
    openWaits,
    say,
    scratch,
    setFolder,
    testHome,
    transcript,
} from "./support.ts";

const NOON = new Date(2026, 9, 14, 12).getTime();

async function mayorHome(parts: Part[] = []) {
    const dir = scratch();
    const { models, faux } = fauxModels();
    const home = await testHome(dir, { models, parts, now: () => NOON });
    return { dir, home, faux, models, house: join(dir, "houses/home/0") };
}

function respond(faux: Faux, ...calls: [string, JsonObject][]) {
    faux.setResponses([
        ...calls.map(([name, args]) =>
            fauxAssistantMessage(fauxToolCall(name, args), {
                stopReason: "toolUse",
            }),
        ),
        fauxAssistantMessage("Done."),
    ]);
}

async function waiting(home: Home): Promise<Readonly<Wait>> {
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    const wait = (await openWaits(home))[0];
    if (wait === undefined) throw new Error("no wait");
    return wait;
}

function rule(home: Home, wait: Readonly<Wait>, choice: Choice, text = "") {
    return answer(home, { wait: wait.id, choice, text, chief: CHIEF }, context);
}

async function results(home: Home): Promise<ToolResultMessage[]> {
    return (await transcript(await mayorChat(home)))
        .flatMap((entry) => entry.model ?? [])
        .filter((message) => message.role === "toolResult");
}

function text(result: ToolResultMessage | undefined): string {
    return (result?.content ?? [])
        .flatMap((part) => (part.type === "text" ? [part.text] : []))
        .join("");
}

function leaves(into: Leave[]): Part {
    return () => ({
        extension: defineExtension({
            name: "test.leaves",
            hooks: [
                hook(ToolTask, {
                    beforeTool: async (_call, api, context) => {
                        const leave = await api.memo<Leave>(LEAVE, context);
                        if (leave !== undefined) into.push(leave);
                        return undefined;
                    },
                }),
            ],
        }),
    });
}

describe("the gate", () => {
    it.each(["edit", "write"])(
        "keeps a held %s's text and preview free of credentials (C32, S7)",
        async (name) => {
            const { home, faux } = await mayorHome();
            const secret = "sk-review-secret-77";
            await home.credentials.modify("openai", async () => ({
                type: "api_key",
                key: secret,
            }));
            const folder = scratch();
            const before = `KEY=${secret}\nTOKEN=${home.credentials.token}\n`;
            writeFileSync(join(folder, ".env"), before);
            await setFolder(home, folder);
            respond(faux, [
                name,
                name === "edit"
                    ? {
                          path: ".env",
                          edits: [{ oldText: before, newText: "gone\n" }],
                      }
                    : { path: ".env", content: "gone\n" },
            ]);

            const submission = await say(home, "Remove the credentials.");
            const wait = await waiting(home);

            expect(wait.about).toBe(
                ".env\nReplace:\nKEY=[REDACTED]\nTOKEN=[REDACTED]\n\nWith:\ngone\n",
            );
            expect(wait.change?.lines).toEqual([
                { mark: "-", text: "KEY=[REDACTED]" },
                { mark: "-", text: "TOKEN=[REDACTED]" },
                { mark: "+", text: "gone" },
            ]);
            const record = await home.harness.snapshot(
                OversightDayDoc,
                "2026-10-14",
                context,
            );
            expect(JSON.stringify(record)).not.toContain(secret);
            expect(JSON.stringify(record)).not.toContain(
                home.credentials.token,
            );
            await rule(home, wait, "deny");
            expect((await submission.wait(context)).status).toBe("done");
            expect(readFileSync(join(folder, ".env"), "utf8")).toBe(before);
        },
    );

    it("judges a nested call too, holding it before it runs", async () => {
        const nested: Part = () => ({
            extension: defineExtension({
                name: "test.nested",
                tools: [
                    defineTool({
                        name: "nested",
                        description: "A nested shell call.",
                        parameters: Type.Object({}),
                        replay: "safe",
                        execute: async (_args, api, context) => {
                            const result = await api.executeTool(
                                "bash",
                                { command: "echo nested > ran", network: true },
                                context,
                            );
                            return {
                                output: [
                                    {
                                        type: "text",
                                        text: result.isError
                                            ? "refused"
                                            : "ran",
                                    },
                                ],
                            };
                        },
                    }),
                ],
            }),
        });
        const { home, faux, house } = await mayorHome([nested]);
        respond(faux, ["nested", {}]);
        const submission = await say(home, "Run nested.");
        const wait = await waiting(home);
        expect(wait).toMatchObject({
            danger: "network",
            about: "echo nested > ran",
        });
        expect(existsSync(join(house, "ran"))).toBe(false);
        await rule(home, wait, "deny");
        expect((await submission.wait(context)).status).toBe("done");
        expect(existsSync(join(house, "ran"))).toBe(false);
        expect(text((await results(home))[0])).toBe("refused");
    });

    it("gives a later holder of the same name none of the earlier holder's grant (C9)", async () => {
        const { home, faux } = await mayorHome();
        const seat = async (number: number) => {
            await buildHouse(home.layout, { hamlet: HOME, number });
            await home.harness.commit(async (tx) => {
                const record = await tx.doc(RecordDoc, HOME, HOME);
                record.agents = record.agents.filter(
                    (agent) => agent.name !== "scout",
                );
                record.agents.push({
                    number,
                    name: "scout",
                    purpose: "Looks around.",
                    model: null,
                    since: NOON,
                });
                record.mayor = number;
            }, context);
        };
        const chiefLine = async (number: number) => {
            const chat = await home.harness.commit(
                async (tx) =>
                    startConversation(home, tx, {
                        hamlet: HOME,
                        number,
                        chief: CHIEF,
                        chat: 1,
                    }),
                context,
            );
            const conversation = await home.harness.conversation(chat, context);
            if (conversation === undefined) throw new Error("no conversation");
            return conversation.submit(
                { type: "input", content: "Fetch." },
                context,
            );
        };
        await seat(7);
        respond(faux, ["bash", { command: "echo curl" }]);
        const first = await chiefLine(7);
        await rule(home, await waiting(home), "always");
        expect((await first.wait(context)).status).toBe("done");
        await seat(9);
        respond(faux, ["bash", { command: "echo curl" }]);
        const later = await chiefLine(9);
        const wait = await waiting(home);
        expect(wait.agent).toEqual({ hamlet: HOME, number: 9 });
        await rule(home, wait, "deny");
        expect((await later.wait(context)).status).toBe("done");
    });

    it("trusts commands for this numbered chat, keeps that trust on a fork, and resets it on resume", async () => {
        const { home, faux } = await mayorHome();
        await setFolder(home, scratch());
        respond(faux, ["bash", { command: "echo one" }]);
        const first = await say(home, "Run.");
        await rule(home, await waiting(home), "always");
        expect((await first.wait(context)).status).toBe("done");
        const chat = await mayorChat(home);
        const entries = await transcript(chat);
        const last = entries.at(-1);
        if (last === undefined) throw new Error("no entry");
        const fork = await chat.fork(
            last.id,
            { ownership: { kind: "ownerless" } },
            context,
        );
        respond(
            faux,
            ["bash", { command: "echo two" }],
            ["write", { path: "a.txt", content: "one" }],
        );
        const forked = await fork.submit(
            { type: "input", content: "Run and write." },
            context,
        );
        const edit = await waiting(home);
        expect(edit.danger).toBe("edit");
        await rule(home, edit, "deny");
        expect((await forked.wait(context)).status).toBe("done");
        const place = await home.harness.snapshot(WithDoc, chat.id, context);
        if (place === undefined) throw new Error("no agent");
        await home.harness.commit((tx) => resetTrust(tx, place), context);
        respond(faux, ["bash", { command: "echo three" }]);
        const resumed = await say(home, "Run again.");
        expect((await waiting(home)).danger).toBe("command");
        await rule(home, await waiting(home), "deny");
        expect((await resumed.wait(context)).status).toBe("done");
    });

    it("holds an edit whose footed path or symlink lands in the folder, with the old and new text", async () => {
        const { home, faux, house } = await mayorHome();
        const folder = scratch();
        await setFolder(home, folder);
        writeFileSync(join(folder, "NOTES.md"), "project");
        symlinkSync(folder, join(house, "project"));
        respond(
            faux,
            ["write", { path: "NOTES.md", content: "new" }],
            [
                "edit",
                {
                    path: join(house, "project/NOTES.md"),
                    edits: [{ oldText: "project", newText: "changed" }],
                },
            ],
        );
        const submission = await say(home, "Change these.");
        const first = await waiting(home);
        expect(first.about).toBe("NOTES.md\nReplace:\nproject\nWith:\nnew");
        expect(first.change).toMatchObject({
            created: false,
            added: 1,
            removed: 1,
        });
        await rule(home, first, "deny");
        const link = await waiting(home);
        expect(link.danger).toBe("edit");
        await rule(home, link, "deny");
        expect((await submission.wait(context)).status).toBe("done");
        expect(readFileSync(join(folder, "NOTES.md"), "utf8")).toBe("project");
        expect(readFileSync(join(house, "NOTES.md"), "utf8")).toBe("# Notes\n");
    });

    it("reads a lowered asking limit between calls and never asks beyond it (H22)", async () => {
        const { home, faux } = await mayorHome();
        await setFolder(home, scratch());
        respond(
            faux,
            ["bash", { command: "echo first" }],
            ["bash", { command: "echo second" }],
        );
        const submission = await say(home, "Run two.");
        const wait = await waiting(home);
        await home.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, HOME, HOME)).asking.refusals = 1;
        }, context);
        await rule(home, wait, "deny");
        expect((await submission.wait(context)).status).toBe("done");
        expect(text((await results(home)).at(-1))).toContain(
            "has refused 1 requests this turn",
        );
        expect(await openWaits(home)).toEqual([]);
    });

    it("runs an allowed network call through the shell's sandbox, with no network standing left afterward", async () => {
        const listener = createServer((_request, response) =>
            response.end("reached"),
        );
        await new Promise<void>((resolve) =>
            listener.listen(0, "127.0.0.1", resolve),
        );
        closeAfter(() => listener.close());
        const port = (listener.address() as AddressInfo).port;
        const { home, faux } = await mayorHome();
        respond(faux, [
            "bash",
            { command: `curl -sS -m 2 http://127.0.0.1:${port}/` },
        ]);
        const submission = await say(home, "Fetch.");
        await rule(home, await waiting(home), "allow");
        expect((await submission.wait(context)).status).toBe("done");
        expect(text((await results(home))[0])).toBe("reached");
        expect(
            grantsOf(await home.harness.snapshot(GrantsDoc, context)),
        ).toEqual([]);
    });

    it("does not close or grant an always request whose agent has no number", async () => {
        const { home } = await mayorHome();
        const id = await home.harness.commit(
            (tx) =>
                raise(
                    tx,
                    {
                        kind: "approval",
                        danger: "network",
                        by: "mayor",
                        agent: null,
                        about: "curl x",
                        change: null,
                        answers: ["allow", "always", "deny"],
                        chat: null,
                        call: null,
                    },
                    NOON,
                ),
            context,
        );
        expect(
            await answer(
                home,
                { wait: id ?? "", choice: "always", chief: CHIEF },
                context,
            ),
        ).toBeUndefined();
        expect(await openWaits(home)).toMatchObject([{ id, asked: 1 }]);
        expect(
            grantsOf(await home.harness.snapshot(GrantsDoc, context)),
        ).toEqual([]);
    });

    it("leaves the wait and grants unchanged when the answer's commit fails, so a chief can answer after reopening", async () => {
        const dir = scratch();
        const { models, faux } = fauxModels();
        let fail = false;
        const home = await testHome(dir, {
            models,
            now: () => NOON,
            openStorage: crashing(() => fail),
        });
        respond(faux, ["bash", { command: "echo curl > ran" }]);
        const submission = await say(home, "Fetch.");
        const wait = await waiting(home);
        fail = true;
        await expect(rule(home, wait, "always")).rejects.toThrow("crash");
        expect(await home.harness.closed).toMatchObject({ reason: "failed" });
        const after = await testHome(dir, { models });
        expect(await openWaits(after)).toEqual([wait]);
        expect(existsSync(join(dir, "houses/home/0/ran"))).toBe(false);
        expect(
            grantsOf(await after.harness.snapshot(GrantsDoc, context)),
        ).toEqual([]);
        await rule(after, await waiting(after), "always");
        expect(
            (
                await (
                    await after.harness.submission(submission.id, context)
                )?.wait(context)
            )?.status,
        ).toBe("done");
        expect(
            grantsOf(await after.harness.snapshot(GrantsDoc, context)),
        ).toMatchObject([
            { agent: { hamlet: "home", number: 0 }, permission: "network" },
        ]);
    });

    it("reads the network switch at every call, even in one run (session/tests.rs:77)", async () => {
        const seen: Leave[] = [];
        let calls = 0;
        const flip: Part = (home) => ({
            extension: defineExtension({
                name: "test.flip",
                hooks: [
                    hook(ToolTask, {
                        beforeTool: async (_call, _api, context) => {
                            calls += 1;
                            await home.harness.commit(async (tx) => {
                                (await tx.doc(SettingsDoc)).allowNet =
                                    calls === 1;
                            }, context);
                            return undefined;
                        },
                    }),
                ],
            }),
        });
        const { home, faux } = await mayorHome([leaves(seen), flip]);
        respond(
            faux,
            ["bash", { command: "echo first" }],
            ["bash", { command: "echo curl" }],
            ["bash", { command: "echo wget" }],
        );
        const submission = await say(home, "Do three things.");
        const wait = await waiting(home);
        expect(wait).toMatchObject({ danger: "network", about: "echo wget" });
        await rule(home, wait, "deny");
        expect((await submission.wait(context)).status).toBe("done");
        expect(seen).toEqual([{ net: false }, { net: true }]);
    });

    it("takes the network from the office named in the current limits (tests.rs:987)", async () => {
        const seen: Leave[] = [];
        const { home, faux } = await mayorHome([leaves(seen)]);
        await home.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, HOME, HOME)).permissions.network = [
                "mayor",
            ];
        }, context);
        respond(faux, ["bash", { command: "echo curl" }]);
        expect((await (await say(home, "Fetch.")).wait(context)).status).toBe(
            "done",
        );
        expect(seen).toEqual([{ net: true }]);
        expect(await openWaits(home)).toEqual([]);
        expect(
            (
                await home.harness.snapshot(
                    OversightDayDoc,
                    "2026-10-14",
                    context,
                )
            )?.events ?? [],
        ).toEqual([]);
        await home.harness.commit(async (tx) => {
            (await tx.doc(LimitsDoc, HOME, HOME)).permissions.network = [
                "founder",
            ];
        }, context);
        respond(faux, ["bash", { command: "echo curl" }]);
        const next = await say(home, "Fetch again.");
        await rule(home, await waiting(home), "deny");
        expect((await next.wait(context)).status).toBe("done");
    });

    it("keeps the current network standing when a manual command is allowed", async () => {
        const seen: Leave[] = [];
        const { home, faux } = await mayorHome([leaves(seen)]);
        await setFolder(home, scratch());
        respond(faux, ["bash", { command: "echo here" }]);
        const submission = await say(home, "Run here.");
        const wait = await waiting(home);
        expect(wait.danger).toBe("command");
        await home.harness.commit(async (tx) => {
            (await tx.doc(SettingsDoc)).allowNet = true;
        }, context);
        await rule(home, wait, "allow");
        expect((await submission.wait(context)).status).toBe("done");
        expect(seen).toEqual([{ net: true }]);
    });

    it("runs edits and commands without asking in auto mode", async () => {
        const { home, faux } = await mayorHome();
        const folder = scratch();
        await setFolder(home, folder, "auto");
        respond(
            faux,
            ["write", { path: "a.txt", content: "one" }],
            ["bash", { command: "touch b.txt" }],
        );
        expect((await (await say(home, "Work.")).wait(context)).status).toBe(
            "done",
        );
        expect(await openWaits(home)).toEqual([]);
        expect(readFileSync(join(folder, "a.txt"), "utf8")).toBe("one");
        expect(existsSync(join(folder, "b.txt"))).toBe(true);
    });

    it("keeps one wait for two chats across a restart, runs neither before an answer, and never raises the answered call again (tests.rs:330, :360)", async () => {
        const { dir, home, faux, models, house } = await mayorHome();
        const command = "echo ran >> ran.txt";
        const call = fauxAssistantMessage(
            fauxToolCall("bash", { command, network: true }),
            { stopReason: "toolUse" },
        );
        faux.setResponses([call, call]);
        const submissions = [await say(home, "Run.")];
        const original = await waiting(home);
        submissions.push(await say(home, "Run too.", "bea"));
        await vi.waitFor(async () =>
            expect(await openWaits(home)).toMatchObject([{ asked: 2 }]),
        );
        await home.close(context);
        const after = await testHome(dir, { models, now: () => NOON + 60_000 });
        const kept = await waiting(after);
        expect(kept).toMatchObject({ id: original.id, since: NOON, asked: 2 });
        expect(existsSync(join(house, "ran.txt"))).toBe(false);
        expect(
            grantsOf(await after.harness.snapshot(GrantsDoc, context)),
        ).toEqual([]);
        faux.appendResponses([
            fauxAssistantMessage("Done."),
            fauxAssistantMessage("Done too."),
        ]);
        await rule(after, kept, "allow");
        for (const submission of submissions)
            expect(
                (
                    await (
                        await after.harness.submission(submission.id, context)
                    )?.wait(context)
                )?.status,
            ).toBe("done");
        expect((await results(after)).map((result) => result.isError)).toEqual([
            false,
        ]);
        expect(readFileSync(join(house, "ran.txt"), "utf8")).toBe("ran\nran\n");
        expect(await openWaits(after)).toEqual([]);
        await after.close(context);
        const again = await testHome(dir, { models });
        expect(await openWaits(again)).toEqual([]);
        expect(readFileSync(join(house, "ran.txt"), "utf8")).toBe("ran\nran\n");
        expect(
            (
                await again.harness.snapshot(
                    OversightDayDoc,
                    "2026-10-14",
                    context,
                )
            )?.events.map((event) => event.type),
        ).toEqual(["raised", "settled"]);
    });

    it.each(["allow", "always", "deny"] as const)(
        "answers %s after a joining chat's held call has ended (H11)",
        async (choice) => {
            const { home, faux, house } = await mayorHome();
            const call = fauxAssistantMessage(
                fauxToolCall("bash", {
                    command: "echo ran >> ran.txt",
                    network: true,
                }),
                { stopReason: "toolUse" },
            );
            faux.setResponses([call, call, fauxAssistantMessage("Done.")]);
            const chief = await say(home, "Run.");
            const wait = await waiting(home);
            const bea = await say(home, "Run too.", "bea");
            await vi.waitFor(async () =>
                expect(await openWaits(home)).toMatchObject([{ asked: 2 }]),
            );
            const joining = await chatOf(
                home,
                { agent: { hamlet: HOME, number: 0 }, chief: "bea" },
                context,
            );
            await joining.abort(context);
            expect((await bea.wait(context)).status).toBe("unanswered");

            await expect(rule(home, wait, choice)).resolves.toMatchObject({
                id: wait.id,
            });
            expect((await chief.wait(context)).status).toBe("done");
            expect(await openWaits(home)).toEqual([]);
            if (choice === "deny") {
                expect(existsSync(join(house, "ran.txt"))).toBe(false);
                expect(text((await results(home))[0])).toContain(
                    "the chief refused it: no reason given",
                );
            } else {
                expect(readFileSync(join(house, "ran.txt"), "utf8")).toBe(
                    "ran\n",
                );
            }
        },
    );

    it("keeps trust and turn refusals across a restart without trusting other dangers", async () => {
        const { dir, home, faux, models } = await mayorHome();
        const folder = scratch();
        await setFolder(home, folder);
        respond(
            faux,
            ["write", { path: "a.txt", content: "one" }],
            ["bash", { command: "echo curl" }],
            ["bash", { command: "echo here" }],
            ["write", { path: "b.txt", content: "two" }],
            ["bash", { command: "echo wget" }],
        );
        const submission = await say(home, "Work.");
        await rule(home, await waiting(home), "always");
        await rule(home, await waiting(home), "deny", "no network");
        const held = await waiting(home);
        expect(held.about).toBe("echo here");
        await home.close(context);
        const after = await testHome(dir, { models });
        await rule(after, await waiting(after), "allow");
        expect(
            (
                await (
                    await after.harness.submission(submission.id, context)
                )?.wait(context)
            )?.status,
        ).toBe("done");
        expect(readFileSync(join(folder, "b.txt"), "utf8")).toBe("two");
        expect(text((await results(after)).at(-1))).toContain(
            "already refused this in this turn (no network)",
        );
        expect(await openWaits(after)).toEqual([]);
    });

    it.each([
        [true, null, "network"],
        [false, null, null],
        [false, "manual", "command"],
    ] as const)(
        "judges a tool added by hand with manifest net %s in mode %s",
        async (net, mode, danger) => {
            const added: Part = () => ({
                extension: defineExtension({
                    name: "test.hand",
                    tools: [
                        defineTool({
                            name: "forecast",
                            description: "A test tool.",
                            parameters: Type.Object({ words: Type.String() }),
                            execute: async (_args, api, context) => ({
                                output: [
                                    {
                                        type: "text",
                                        text: JSON.stringify(
                                            await api.memo<Leave>(
                                                LEAVE,
                                                context,
                                            ),
                                        ),
                                    },
                                ],
                            }),
                        }),
                    ],
                }),
            });
            const { home, faux } = await mayorHome([added]);
            await home.harness.commit(async (tx) => {
                (await tx.doc(ManifestsDoc)).tools.push({
                    name: "forecast",
                    description: "A test tool.",
                    dir: home.layout.tools,
                    exec: ["sh"],
                    parameters: {},
                    net,
                    ro: [],
                    rw: [],
                    env: {},
                    wallSecs: null,
                    cpuSecs: null,
                });
            }, context);
            if (mode !== null) await setFolder(home, scratch(), mode);
            respond(faux, ["forecast", { words: "curl" }]);
            const submission = await say(home, "Forecast.");
            if (danger !== null) {
                const wait = await waiting(home);
                expect(wait).toMatchObject({
                    danger,
                    about: 'forecast {"words":"curl"}',
                });
                await rule(home, wait, "allow");
            }
            expect((await submission.wait(context)).status).toBe("done");
            expect(text((await results(home))[0])).toBe(
                net ? '{"net":true}' : '{"net":false}',
            );
            expect(await openWaits(home)).toEqual([]);
        },
    );

    it("shows a write over a file that is not UTF-8 as a replacement, never as a new file", async () => {
        const { home, faux } = await mayorHome();
        const folder = scratch();
        const path = join(folder, "logo.png");
        const bytes = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0xff, 0xfe, 0x00]);
        writeFileSync(path, bytes);
        await setFolder(home, folder);
        respond(faux, ["write", { path: "logo.png", content: "x" }]);
        const submission = await say(home, "Replace the logo.");

        const wait = await waiting(home);
        expect(wait.about).toBe(
            "logo.png\nReplace:\n[a file that is not UTF-8 text, 7 bytes]\nWith:\nx",
        );
        expect(wait.change).toMatchObject({ created: false });
        expect(wait.change?.count).not.toContain("New file");
        expect(readFileSync(path)).toEqual(bytes);
        await rule(home, wait, "allow");
        expect((await submission.wait(context)).status).toBe("done");
        expect(readFileSync(path, "utf8")).toBe("x");
    });

    it("shows a write over an empty file as a replacement", async () => {
        const { home, faux } = await mayorHome();
        const folder = scratch();
        writeFileSync(join(folder, "empty.txt"), "");
        await setFolder(home, folder);
        respond(faux, ["write", { path: "empty.txt", content: "x" }]);
        const submission = await say(home, "Replace the empty file.");

        const wait = await waiting(home);
        expect(wait.about).toBe("empty.txt\nReplace:\n\nWith:\nx");
        expect(wait.change).toMatchObject({ created: false });
        await rule(home, wait, "deny");
        expect((await submission.wait(context)).status).toBe("done");
        expect(readFileSync(join(folder, "empty.txt"), "utf8")).toBe("");
    });

    it("holds folder edits but frees house edits and reads, and always trusts only edits in that chat (tests.rs:1224)", async () => {
        const { home, faux, house } = await mayorHome();
        const folder = scratch();
        await setFolder(home, folder);
        respond(
            faux,
            [
                "edit",
                {
                    path: "NOTES.md",
                    edits: [{ oldText: "# Notes", newText: "# Notes\nTea." }],
                },
            ],
            ["write", { path: "skills/tea/SKILL.md", content: "Make tea." }],
            ["write", { path: "a.txt", content: "one" }],
            ["read", { path: "a.txt" }],
            ["find", {}],
            ["search", { pattern: "one" }],
            ["write", { path: "b.txt", content: "two" }],
            ["bash", { command: "touch c.txt" }],
            [
                "edit",
                { path: "a.txt", edits: [{ oldText: "one", newText: "ONE" }] },
            ],
            ["bash", { command: "touch d.txt" }],
        );
        const submission = await say(home, "Work on these.");
        const edit = await waiting(home);
        expect(readFileSync(join(house, "NOTES.md"), "utf8")).toBe(
            "# Notes\nTea.\n",
        );
        expect(readFileSync(join(house, "skills/tea/SKILL.md"), "utf8")).toBe(
            "Make tea.",
        );
        expect(existsSync(join(folder, "skills"))).toBe(false);
        expect(edit).toMatchObject({
            danger: "edit",
            about: "a.txt\nA new file:\none",
            change: { path: "a.txt", created: true, added: 1, removed: 0 },
        });
        expect(existsSync(join(folder, "a.txt"))).toBe(false);
        await rule(home, edit, "always");
        const command = await waiting(home);
        expect(command).toMatchObject({
            danger: "command",
            about: "touch c.txt",
        });
        expect(readFileSync(join(folder, "b.txt"), "utf8")).toBe("two");
        expect(readFileSync(join(house, "NOTES.md"), "utf8")).toBe(
            "# Notes\nTea.\n",
        );
        expect(existsSync(join(folder, "NOTES.md"))).toBe(false);
        await rule(home, command, "allow");
        const next = await waiting(home);
        expect(next.about).toBe("touch d.txt");
        await rule(home, next, "deny");
        expect((await submission.wait(context)).status).toBe("done");
        expect(readFileSync(join(folder, "a.txt"), "utf8")).toBe("ONE");
        expect(existsSync(join(folder, "c.txt"))).toBe(true);
        expect(existsSync(join(folder, "d.txt"))).toBe(false);
        respond(faux, ["write", { path: "e.txt", content: "three" }]);
        const bea = await say(home, "Write here too.", "bea");
        await rule(home, await waiting(home), "deny");
        expect((await bea.wait(context)).status).toBe("done");
        expect(existsSync(join(folder, "e.txt"))).toBe(false);
    });

    it("keeps an always network grant by number until revoked, and a second revoke changes nothing (tests.rs:222)", async () => {
        const { home, faux, house } = await mayorHome();
        respond(faux, ["bash", { command: "echo curl > first" }]);
        const first = await say(home, "Fetch.");
        await rule(home, await waiting(home), "always");
        expect((await first.wait(context)).status).toBe("done");
        expect(
            grantsOf(await home.harness.snapshot(GrantsDoc, context)),
        ).toEqual([
            {
                agent: { hamlet: "home", number: 0 },
                name: "mayor",
                permission: "network",
                by: CHIEF,
            },
        ]);
        respond(faux, ["bash", { command: "echo wget > second" }]);
        expect(
            (await (await say(home, "Fetch more.")).wait(context)).status,
        ).toBe("done");
        expect(readFileSync(join(house, "second"), "utf8")).toBe("wget\n");
        expect(await openWaits(home)).toEqual([]);
        const takeBack = () =>
            home.harness.commit(
                (tx) =>
                    revoke(
                        tx,
                        { hamlet: "home", number: 0 },
                        "network",
                        CHIEF,
                        NOON,
                    ),
                context,
            );
        expect(await takeBack()).toBe(true);
        const revoked = await home.harness.snapshot(GrantsDoc, context);
        expect(await takeBack()).toBe(false);
        expect(await home.harness.snapshot(GrantsDoc, context)).toEqual(
            revoked,
        );
        respond(faux, ["bash", { command: "echo curl > third" }]);
        const third = await say(home, "Fetch again.");
        await rule(home, await waiting(home), "deny");
        expect((await third.wait(context)).status).toBe("done");
        expect(existsSync(join(house, "third"))).toBe(false);
    });

    it("remembers a refused kind through separate model responses, but asks again on the next turn (tests.rs:181)", async () => {
        const { home, faux, house } = await mayorHome();
        respond(
            faux,
            ["bash", { command: "echo curl > first" }],
            ["bash", { command: "echo wget > second" }],
        );
        const submission = await say(home, "Fetch.");
        await rule(
            home,
            await waiting(home),
            "deny",
            "  not on this machine  ",
        );
        expect((await submission.wait(context)).status).toBe("done");
        expect((await results(home)).map(text)).toEqual([
            "<harness>\n[error] Tool call blocked: the chief refused it: not on this machine\n</harness>",
            "<harness>\n[error] Tool call blocked: the chief already refused this in this turn (not on this machine). Do not ask again: go on without it, or report that you could not.\n</harness>",
        ]);
        expect(existsSync(join(house, "first"))).toBe(false);
        expect(existsSync(join(house, "second"))).toBe(false);
        expect(await openWaits(home)).toEqual([]);
        respond(faux, ["bash", { command: "echo wget > third" }]);
        const next = await say(home, "Try again.");
        await rule(home, await waiting(home), "deny");
        expect((await next.wait(context)).status).toBe("done");
        expect(text((await results(home)).at(-1))).toBe(
            "<harness>\n[error] Tool call blocked: the chief refused it: no reason given\n</harness>",
        );
    });

    it("ends the asking after three refused approvals in one turn (tests.rs:296)", async () => {
        const { home, faux } = await mayorHome();
        const folder = scratch();
        await setFolder(home, folder);
        respond(
            faux,
            ["bash", { command: "touch a" }],
            ["bash", { command: "touch a" }],
            ["bash", { command: "touch b" }],
            ["write", { path: "c", content: "one" }],
            ["bash", { command: "touch d" }],
        );
        const submission = await say(home, "Do four things.");
        for (const about of ["touch a", "touch b", "c\nA new file:\none"]) {
            const wait = await waiting(home);
            expect(wait.about).toBe(about);
            await rule(home, wait, "deny", "leave it");
        }
        expect((await submission.wait(context)).status).toBe("done");
        expect(text((await results(home))[1])).toContain(
            "already refused this in this turn (leave it)",
        );
        expect(text((await results(home)).at(-1))).toBe(
            "<harness>\n[error] Tool call blocked: the chief has refused 3 requests this turn and will not be asked again in it. Stop and report what you could not do.\n</harness>",
        );
        expect(await openWaits(home)).toEqual([]);
        for (const name of ["a", "b", "c", "d"])
            expect(existsSync(join(folder, name))).toBe(false);
    });

    it("holds a network call, ignores an answer it does not offer, and grants the network only to the allowed call (oversight/tests.rs:122)", async () => {
        const seen: Leave[] = [];
        const { home, faux, house } = await mayorHome([leaves(seen)]);
        const command = "printf ran > ran.txt";
        respond(
            faux,
            ["bash", { command, network: true }],
            ["bash", { command: "echo stayed" }],
        );
        const submission = await say(home, "Run it.");
        const wait = await waiting(home);

        expect(wait).toMatchObject({
            kind: "approval",
            danger: "network",
            by: "mayor",
            agent: { hamlet: "home", number: 0 },
            about: command,
            answers: ["allow", "always", "deny"],
            asked: 1,
        });
        expect(existsSync(join(house, "ran.txt"))).toBe(false);
        expect(await results(home)).toEqual([]);
        expect(await rule(home, wait, "go")).toBeUndefined();
        expect(await openWaits(home)).toEqual([wait]);
        await rule(home, wait, "allow", "  this once  ");
        expect((await submission.wait(context)).status).toBe("done");
        expect(readFileSync(join(house, "ran.txt"), "utf8")).toBe("ran");
        expect(seen).toEqual([{ net: true }, { net: false }]);
        expect(await openWaits(home)).toEqual([]);
        expect(
            (
                await home.harness.snapshot(
                    OversightDayDoc,
                    "2026-10-14",
                    context,
                )
            )?.events,
        ).toEqual([
            {
                type: "raised",
                at: NOON,
                id: wait.id,
                kind: "approval",
                by: "mayor",
                agent: { hamlet: "home", number: 0 },
                about: command,
            },
            {
                type: "settled",
                at: NOON,
                id: wait.id,
                how: "allow",
                text: "this once",
                chief: CHIEF,
            },
        ]);
        expect(await rule(home, wait, "deny")).toBeUndefined();
    });
});
