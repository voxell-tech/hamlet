import { expect, it } from "vitest";
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

it("finds an ending recorded on an earlier day after the wall clock goes back", async () => {
    const raised = new Date(2026, 9, 16, 12).getTime();
    const settled = new Date(2026, 9, 14, 12).getTime();
    const later = new Date(2026, 9, 17, 12).getTime();
    const home = await testHome(scratch(), { now: () => later });
    const id = await home.harness.commit(
        (tx) =>
            raiseFault(
                tx,
                { by: "mayor", agent: MAYOR, about: "clock moved", chat: null },
                raised,
            ),
        context,
    );
    await home.harness.commit(
        (tx) =>
            settle(
                tx,
                id,
                { how: "dismiss", text: "", chief: home.chief },
                settled,
            ),
        context,
    );
    expect(await ended(home.harness, [id], context, later)).toEqual(
        new Map([[id, "dismiss"]]),
    );
});

it("reads existing unindexed day records without changing their stored version", async () => {
    const { OversightDoc } = await import("../src/oversight/waits.ts");
    const at = new Date(2026, 9, 14, 12).getTime();
    const home = await testHome(scratch(), { now: () => at });
    const id = await home.harness.commit(
        (tx) =>
            raiseFault(
                tx,
                { by: "mayor", agent: MAYOR, about: "old record", chat: null },
                at,
            ),
        context,
    );
    await home.harness.commit(async (tx) => {
        await settle(
            tx,
            id,
            { how: "dismiss", text: "", chief: home.chief },
            at,
        );
        delete (await tx.doc(OversightDoc)).days;
    }, context);
    expect(await ended(home.harness, [id], context, at)).toEqual(
        new Map([[id, "dismiss"]]),
    );
    expect(await ended(home.harness, ["", "unknown"], context, at)).toEqual(
        new Map(),
    );
});
