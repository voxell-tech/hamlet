import { expect, it } from "vitest";
import { localDay } from "../src/home/clock.ts";
import { OversightDayDoc, OversightDoc } from "../src/oversight/waits.ts";
import {
    ended,
    forAgent,
    forChat,
    open,
    raiseFault,
    settle,
    view,
} from "../src/oversight.ts";
import { RecordDoc } from "../src/record.ts";
import { context, MAYOR, scratch, testHome } from "./support.ts";

it("queries waits in raise order across a backwards clock, distinguishes chats, and projects a past chat", async () => {
    const home = await testHome(scratch(), {});
    const chat = { hamlet: "home", number: 0, chief: home.chief, chat: 2 };
    for (const [about, at, place] of [
        ["first", 2000, chat],
        ["second", 1000, { ...chat, chief: "bea" }],
        ["third", 1000, { ...chat, hamlet: "far" }],
    ] as const) {
        await home.harness.commit(
            (tx) =>
                raiseFault(
                    tx,
                    {
                        by: "mayor",
                        agent: { hamlet: place.hamlet, number: 0 },
                        about,
                        chat: place,
                    },
                    at,
                ),
            context,
        );
    }
    const waits = await open(home.harness, context);
    expect(waits.map((wait) => wait.about)).toEqual([
        "first",
        "second",
        "third",
    ]);
    expect(forAgent(waits, MAYOR).map((wait) => wait.about)).toEqual([
        "first",
        "second",
    ]);
    expect(
        forChat(waits, { agent: MAYOR, chief: home.chief }).map(
            (wait) => wait.about,
        ),
    ).toEqual(["first"]);
    const first = waits[0];
    if (!first) throw new Error("missing wait");
    const record = await home.harness.snapshot(RecordDoc, "home", context);
    if (!record) throw new Error("missing record");
    expect(
        view(
            first,
            { home: record },
            new Map([["home:0", { [home.chief]: 3 }]]),
        ),
    ).toMatchObject({
        chat: { with: { hamlet: "home", agent: "mayor" }, past: 2 },
        says: "",
    });
});

it("finds endings across midnight and ignores open or unknown ids", async () => {
    const at = new Date(2026, 9, 14, 23, 59).getTime();
    const later = new Date(2026, 9, 16, 0, 1).getTime();
    const home = await testHome(scratch(), { now: () => later });
    const id = await home.harness.commit(
        (tx) =>
            raiseFault(
                tx,
                { by: "mayor", agent: MAYOR, about: "failed", chat: null },
                at,
            ),
        context,
    );
    await home.harness.commit(
        (tx) =>
            settle(
                tx,
                id,
                { how: "dismiss", text: "", chief: home.chief },
                later,
            ),
        context,
    );
    expect(
        await ended(home.harness, [id, "unknown", ""], context, later),
    ).toEqual(new Map([[id, "dismiss"]]));
});

it("keeps the stored oversight shape limited to the raise counter and open waits", async () => {
    const home = await testHome(scratch(), {});
    const id = await home.harness.commit(
        (tx) =>
            raiseFault(
                tx,
                { by: "mayor", agent: MAYOR, about: "failed", chat: null },
                home.now(),
            ),
        context,
    );
    await home.harness.commit(
        (tx) =>
            settle(
                tx,
                id,
                { how: "dismiss", text: "", chief: home.chief },
                home.now(),
            ),
        context,
    );
    expect(await home.harness.snapshot(OversightDoc, context)).toEqual({
        raised: 1,
        waits: {},
    });
});

it.each(["before", "after"] as const)(
    "finds a settlement just %s the scan's midnight boundary without an index",
    async (side) => {
        const at =
            side === "before"
                ? new Date(2026, 9, 15, 0, 0, 0, 1).getTime()
                : new Date(2026, 9, 15, 23, 59, 59, 998).getTime();
        const settledAt = at + (side === "before" ? -2 : 3);
        const home = await testHome(scratch(), { now: () => at });
        const id = `${at}-1`;
        const day = localDay(settledAt);
        await home.harness.commit(async (tx) => {
            (await tx.doc(OversightDayDoc, day, day)).events.push({
                type: "settled",
                at: settledAt,
                id,
                how: "dismiss",
                text: "",
                chief: home.chief,
            });
        }, context);
        expect(await ended(home.harness, [id], context, at + 1)).toEqual(
            new Map([[id, "dismiss"]]),
        );
    },
);

it("orders the second wait before the tenth by raise number, not lexical id or clock time", async () => {
    const home = await testHome(scratch(), {});
    const ids = [];
    for (let number = 1; number <= 10; number++) {
        ids.push(
            await home.harness.commit(
                (tx) =>
                    raiseFault(
                        tx,
                        {
                            by: "mayor",
                            agent: MAYOR,
                            about: `fault ${number}`,
                            chat: null,
                        },
                        number === 1 ? 2000 : 1000,
                    ),
                context,
            ),
        );
    }
    expect((await open(home.harness, context)).map((wait) => wait.id)).toEqual(
        ids,
    );
});
