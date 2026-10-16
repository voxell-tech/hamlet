import { expect, test } from "vitest";
import { LimitsDoc } from "../src/limits.ts";
import {
    admit,
    carryRoad,
    decisionText,
    layRoad,
    mayDecide,
    RecordDoc,
    roadsOf,
    seat,
    takeUpRoad,
    VotesDoc,
} from "../src/record.ts";
import { context, fauxModels, scratch, testHome } from "./support.ts";

test("roads are charged once, capped, and office roads follow a change of mayor", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    await home.harness.commit(async (tx) => {
        for (const name of ["scout", "smith", "poet"])
            expect(
                (
                    await admit(tx, "home", {
                        name,
                        purpose: "Works",
                        reason: "Needed",
                        by: "mayor",
                        at: 100,
                    })
                ).ok,
            ).toBe(true);
        const options = {
            by: "chief:chief",
            reason: "Needed",
            at: 101,
            task: null,
        };
        (await tx.doc(LimitsDoc, "home", "home")).mostRoads = 1;
        expect(
            await layRoad(tx, "home", "scout", "smith", options),
        ).toMatchObject({ ok: true, value: { a: 1, b: 2 } });
        expect(
            await layRoad(tx, "home", "scout", "smith", options),
        ).toMatchObject({ ok: false, error: { kind: "road-exists" } });
        expect(
            await layRoad(tx, "home", "scout", "poet", options),
        ).toMatchObject({ ok: false, error: { kind: "roads-full" } });
        expect(
            await takeUpRoad(tx, "home", "mayor", "scout", options),
        ).toMatchObject({ ok: false, error: { kind: "office-road" } });
        expect((await seat(tx, "home", 1, options)).ok).toBe(true);
        expect(roadsOf(await tx.doc(RecordDoc, "home", "home"))).toEqual(
            expect.arrayContaining([
                expect.objectContaining({ a: "office", b: 0 }),
            ]),
        );
        expect(
            await takeUpRoad(tx, "home", "smith", "poet", options),
        ).toMatchObject({ ok: false, error: { kind: "road-missing" } });
    }, context);
});

test("decision checks read current vote limits in the same transaction", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    await home.harness.commit(async (tx) => {
        (await tx.doc(VotesDoc, "home", "home")).votes.push({
            id: 7,
            motion: { kind: "replace_mayor", candidate: 0 },
            by: { chief: "chief" },
            reason: "Try",
            mayor: 0,
            voters: [],
            answer: null,
            ballots: [],
            standing: "open",
            called: 100,
            task: "task",
        });
        const held = await mayDecide(tx, "home", 101, "admit");
        expect(held).toMatchObject({
            ok: false,
            error: { kind: "vote-open", vote: 7 },
        });
        if (!held.ok)
            expect(decisionText(held.error)).toContain(
                "nothing else is decided",
            );
        (await tx.doc(LimitsDoc, "home", "home")).votes.openMinutes = 0;
        expect(await mayDecide(tx, "home", 101, "admit")).toEqual({
            ok: true,
            value: undefined,
        });
    }, context);
});

test("the first use of an implied office road is counted in its stored road", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    await home.harness.commit(async (tx) => {
        await admit(tx, "home", {
            name: "scout",
            purpose: "Works",
            by: "mayor",
            reason: "Needed",
            at: 100,
        });
        const r = await tx.doc(RecordDoc, "home", "home");
        carryRoad(r, 0, 1, 101);
        carryRoad(r, 1, 0, 102);
        expect(r.roads).toMatchObject([
            { a: "office", b: 1, messages: 2, lastFrom: 1, lastUsed: 102 },
        ]);
    }, context);
});
