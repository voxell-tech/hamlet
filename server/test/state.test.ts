import { randomUUID } from "node:crypto";
import fs from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { scriptedModels } from "../src/models.ts";
import { parseServer } from "../src/protocol.ts";
import { stateOf, watchState } from "../src/state.ts";
import {
    agents,
    changes,
    chat,
    joined,
    mayor,
    request,
    say,
    served,
} from "./connection-support.ts";
import {
    context,
    fauxModels,
    scratch,
    setFolder,
    testHome,
} from "./support.ts";

it("serves a house even when recursive filesystem watches are exhausted", async () => {
    const { models } = fauxModels();
    const home = await testHome(scratch(), { models });
    const original = fs.watch;
    const watching = vi
        .spyOn(fs, "watch")
        .mockImplementation((...args: Parameters<typeof fs.watch>) => {
            const options = args[1] as unknown as { recursive?: boolean };
            if (typeof options === "object" && options?.recursive)
                throw Object.assign(
                    new Error("System limit for file watchers reached"),
                    { code: "ENOSPC" },
                );
            return Reflect.apply(original, fs, args);
        });
    syncBuiltinESMExports();
    try {
        const state = await watchState(home, () => {}, context);
        try {
            expect((await state.snapshot()).agents[0]?.name).toBe("founder");
        } finally {
            await state.stop();
        }
    } finally {
        watching.mockRestore();
        syncBuiltinESMExports();
    }
});

it("lists new house skills on demand after its short cache expires", async () => {
    const { models } = fauxModels();
    const home = await testHome(scratch(), { models });
    const clock = vi.spyOn(Date, "now").mockReturnValue(1000);
    try {
        const at = { epoch: 1, revision: 0 };
        expect((await stateOf(home, at, context)).agents[0]?.skills).toEqual(
            [],
        );
        const skill = join(home.layout.houses, "home/0/skills/new");
        await mkdir(skill);
        await writeFile(join(skill, "SKILL.md"), "---\nname: new\n---\n");
        clock.mockReturnValue(2001);
        expect((await stateOf(home, at, context)).agents[0]?.skills).toEqual([
            "new",
        ]);
    } finally {
        clock.mockRestore();
    }
});

it("lists only the chief's deeds the server carries out, while accepting older states without them (H26, S6)", async () => {
    const run = await served({
        models: scriptedModels({ responses: [] }),
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    expect(run.state).toHaveProperty("deeds", [
        "new",
        "resume",
        "retry",
        "agent",
        "hamlet",
        "to",
        "stop",
        "queue",
        "allow",
        "always",
        "deny",
        "go",
        "withdraw",
        "reply",
        "dismiss",
        "undo",
        "grants",
        "revoke",
        "remove",
        "merge",
        "road",
        "unroad",
        "give",
        "folder",
        "mode",
        "reset-budget",
        "restore",
        "erase",
        "record",
        "budget",
        "reflect",
        "mayor",
        "vote",
        "votes",
        "overturn",
        "hamlets",
        "found",
        "remove-hamlet",
        "restore-hamlet",
        "unshelve",
        "erase-hamlet",
        "name-hamlet",
        "move-hamlet",
        "chief",
    ]);
    expect(parseServer(JSON.stringify(run.state))).toEqual({
        message: run.state,
    });
    const older = { ...run.state };
    Reflect.deleteProperty(older, "deeds");
    expect(parseServer(JSON.stringify(older))).toEqual({ message: older });
});

it("names a wait's current chat without past and pins it only after the chief starts a new conversation (S5, C24)", async () => {
    const { models, faux } = fauxModels();
    const run = await served({ models, env: { HAMLET_REFLECT_EVERY: "0" } });
    await setFolder(run.home, scratch());
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo held" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("Done."),
    ]);
    say(run.connection, "Run.");
    await vi.waitFor(() =>
        expect(changes(run.messages).some((item) => item.type === "wait")).toBe(
            true,
        ),
    );
    const waits = () =>
        changes(run.messages).flatMap((item) =>
            item.type === "wait" ? [item.wait] : [],
        );
    expect(waits().at(-1)?.chat).toEqual(chat);

    await request(run.connection, run.messages, {
        type: "new",
        id: randomUUID(),
        to: mayor,
    });

    await vi.waitFor(() =>
        expect(waits().at(-1)?.chat).toEqual({ with: mayor, past: 1 }),
    );
});

it("shows the budget and follows the mayor's thinking, fetching, failure and rest without opening its chat (S5)", async () => {
    const run = await served({
        env: { HAMLET_NET: "1", HAMLET_REFLECT_EVERY: "0" },
        models: scriptedModels({
            tokensPerSecond: 20,
            responses: [
                {
                    thinking: "Let me see.",
                    calls: [
                        {
                            name: "bash",
                            arguments: {
                                command:
                                    "# curl example.invalid\nsleep 0.3; echo broken; exit 7",
                            },
                        },
                    ],
                },
                "Could not fetch.",
            ],
        }),
    });
    expect(run.state.hamlets[0]?.budget).toMatchObject({
        summary: "Hamlet today: $0.00 of $5.00",
        remains: "$5.00",
        left: 1,
    });
    expect(run.state.providers).toMatchObject([
        { id: "faux", configured: true },
    ]);
    say(run.connection, "Look.");
    await vi.waitFor(() =>
        expect(agents(run.messages)).toContainEqual(
            expect.objectContaining({
                busy: true,
                activity: {
                    doing: "think",
                    doingWords: "Thinking",
                    step: 1,
                    failures: 0,
                },
            }),
        ),
    );
    const late = await joined(run.server.url, run.home.credentials.token);
    expect(late.state.agents[0]).toMatchObject({
        busy: true,
        activity: { doing: "think", step: 1 },
    });
    await vi.waitFor(
        () => {
            expect(agents(run.messages)).toContainEqual(
                expect.objectContaining({
                    activity: {
                        doing: "fetch",
                        doingWords: "Fetching from the network",
                        step: 2,
                        failures: 0,
                    },
                    saying: { text: "", thought: true },
                }),
            );
            expect(agents(run.messages)).toContainEqual(
                expect.objectContaining({
                    activity: {
                        doing: "think",
                        doingWords: "Thinking",
                        step: 3,
                        failures: 1,
                    },
                }),
            );
            expect(agents(run.messages).at(-1)).toMatchObject({
                busy: false,
                activity: { doing: "rest", step: 0, failures: 0 },
                saying: { text: "Could not fetch.", thought: false },
            });
        },
        { timeout: 10_000 },
    );
    const revisions = run.messages.flatMap((message) =>
        message.type === "change" ? [message.revision] : [],
    );
    expect(
        revisions.every((revision, i) => revision > (revisions[i - 1] ?? 0)),
    ).toBe(true);
}, 20_000);
