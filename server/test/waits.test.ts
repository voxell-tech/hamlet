import { writeFileSync } from "node:fs";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { describe, expect, it, vi } from "vitest";
import { current, type With } from "../src/chats.ts";
import {
    dismiss,
    lapse,
    OversightDayDoc,
    raise,
    raiseFault,
    settle,
} from "../src/oversight.ts";
import {
    CHIEF,
    context,
    fauxModels,
    MAYOR,
    openWaits,
    say,
    scratch,
    setFolder,
    testHome,
    transcript,
} from "./support.ts";

/** Noon of 14 October 2026, local time. */
const NOON = new Date(2026, 9, 14, 12).getTime();

const list = fauxAssistantMessage(fauxToolCall("bash", { command: "ls" }), {
    stopReason: "toolUse",
});

describe("what waits for a chief", () => {
    it("joins the same call from another chat, keeps its time and count across a restart, and records how it ended", async () => {
        const dir = scratch();
        let now = NOON;
        const { models, faux } = fauxModels();
        const options = { models, now: () => now };
        const first = await testHome(dir, options);
        const folder = scratch();
        writeFileSync(join(folder, "listed.txt"), "");
        await setFolder(first, folder, "manual");
        faux.setResponses([list, list]);

        const asked = [await say(first, "List the folder.")];
        // The chief's chat raises the wait, and bea's joins it.
        await vi.waitFor(async () =>
            expect(await openWaits(first)).toHaveLength(1),
        );
        asked.push(await say(first, "List the folder too.", "bea"));
        await vi.waitFor(async () =>
            expect(await openWaits(first)).toMatchObject([{ asked: 2 }]),
        );
        await first.close(context);
        now = NOON + 60_000;
        const home = await testHome(dir, options);

        const [wait] = await openWaits(home);
        expect(wait).toMatchObject({
            kind: "approval",
            danger: "command",
            by: "mayor",
            agent: MAYOR,
            about: "ls",
            answers: ["allow", "always", "deny"],
            since: NOON,
            asked: 2,
            chat: { hamlet: "home", number: 0, chief: CHIEF, chat: 1 },
        });
        expect(wait?.calls).toHaveLength(2);

        faux.appendResponses([
            fauxAssistantMessage("Listed."),
            fauxAssistantMessage("Listed too."),
        ]);
        await home.harness.commit(
            (tx) =>
                settle(
                    tx,
                    wait?.id ?? "",
                    { how: "allow", text: "", chief: CHIEF },
                    home.now(),
                ),
            context,
        );
        for (const submission of asked) {
            const settled = await (
                await home.harness.submission(submission.id, context)
            )?.wait(context);
            expect(settled?.status).toBe("done");
        }

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
                id: wait?.id,
                kind: "approval",
                by: "mayor",
                agent: MAYOR,
                about: "ls",
            },
            {
                type: "settled",
                at: NOON + 60_000,
                id: wait?.id,
                how: "allow",
                text: "",
                chief: CHIEF,
            },
        ]);
        for (const chief of [CHIEF, "bea"]) {
            const chat = await current(home, { agent: MAYOR, chief }, context);
            const result = (await transcript(chat)).find(
                (entry) => entry.kind === "pi.tool-result",
            );
            expect(JSON.stringify(result?.model)).toContain("listed.txt");
        }
    });

    it("lapses a chat's waits as stopped, but for its faults, and as rewound", async () => {
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models, now: () => NOON });
        await setFolder(home, scratch(), "manual");
        faux.setResponses([list, fauxAssistantMessage("Stopped.")]);
        const chat = { agent: MAYOR, chief: CHIEF };
        const place = { hamlet: "home", number: 0, chief: CHIEF, chat: 1 };

        const submission = await say(home, "List the folder.");
        await vi.waitFor(async () =>
            expect(await openWaits(home)).toHaveLength(1),
        );
        await home.harness.commit(
            (tx) =>
                raiseFault(
                    tx,
                    { by: "mayor", agent: MAYOR, about: "boom", chat: place },
                    NOON,
                ),
            context,
        );
        await home.harness.commit(
            (tx) =>
                lapse(tx, chat, { how: "stopped", chief: CHIEF }, home.now()),
            context,
        );

        expect((await submission.wait(context)).status).toBe("done");
        expect(await openWaits(home)).toMatchObject([
            { kind: "fault", about: "boom" },
        ]);
        await home.harness.commit(
            (tx) =>
                lapse(
                    tx,
                    chat,
                    { how: "rewound", chief: null, since: NOON },
                    home.now(),
                ),
            context,
        );
        expect(await openWaits(home)).toEqual([]);
        const events =
            (
                await home.harness.snapshot(
                    OversightDayDoc,
                    "2026-10-14",
                    context,
                )
            )?.events ?? [];
        expect(
            events.flatMap((event) =>
                event.type === "settled" ? [[event.how, event.chief]] : [],
            ),
        ).toEqual([
            ["stopped", CHIEF],
            ["rewound", null],
        ]);
    });

    it("lapses only the waits of the chat it is given, and on a rewind only those raised since the message", async () => {
        const { models } = fauxModels();
        const home = await testHome(scratch(), { models, now: () => NOON });
        const own = { hamlet: "home", number: 0, chief: CHIEF, chat: 1 };
        const raised = (about: string, chat: With | null, at: number) =>
            home.harness.commit(
                (tx) =>
                    raise(
                        tx,
                        {
                            kind: "approval",
                            danger: "command",
                            by: "mayor",
                            agent: MAYOR,
                            about,
                            change: null,
                            answers: ["allow", "always", "deny"],
                            chat,
                            call: null,
                        },
                        at,
                    ),
                context,
            );
        await raised("before", own, NOON - 1);
        await raised("since", own, NOON);
        await raised("bea's", { ...own, chief: "bea" }, NOON);
        await raised("another agent's", { ...own, number: 1 }, NOON);
        await raised("another hamlet's", { ...own, hamlet: "far" }, NOON);
        await raised("no chat's", null, NOON);
        const chat = { agent: MAYOR, chief: CHIEF };
        const left = async () =>
            (await openWaits(home)).map((wait) => wait.about).sort();

        await home.harness.commit(
            (tx) =>
                lapse(
                    tx,
                    chat,
                    { how: "rewound", chief: CHIEF, since: NOON },
                    NOON,
                ),
            context,
        );
        expect(await left()).toEqual(
            [
                "before",
                "bea's",
                "another agent's",
                "another hamlet's",
                "no chat's",
            ].sort(),
        );
        await home.harness.commit(
            (tx) => lapse(tx, chat, { how: "stopped", chief: CHIEF }, NOON),
            context,
        );
        expect(await left()).toEqual(
            [
                "bea's",
                "another agent's",
                "another hamlet's",
                "no chat's",
            ].sort(),
        );
    });

    it("asks a fault again when it repeats, until a chief dismisses it", async () => {
        const { models } = fauxModels();
        const home = await testHome(scratch(), { models, now: () => NOON });
        const fault = {
            by: "mayor",
            agent: MAYOR,
            about: "The model is not available.",
            chat: null,
        };

        const ids = [
            await home.harness.commit(
                (tx) => raiseFault(tx, fault, NOON),
                context,
            ),
            await home.harness.commit(
                (tx) => raiseFault(tx, fault, NOON + 1),
                context,
            ),
        ];

        expect(ids[1]).toBe(ids[0]);
        expect(await openWaits(home)).toMatchObject([
            { kind: "fault", answers: ["dismiss"], asked: 2, since: NOON },
        ]);
        await home.harness.commit(
            (tx) => dismiss(tx, ids[0] ?? "", CHIEF, NOON + 2),
            context,
        );
        expect(await openWaits(home)).toEqual([]);
    });
});
