import { expect, test } from "vitest";
import { InstructionsDoc } from "../src/chats/instructions.ts";
import { localDay } from "../src/home/clock.ts";
import { LimitsDoc } from "../src/limits.ts";
import {
    admit,
    agentKey,
    byWords,
    carryRoad,
    depart,
    endName,
    layRoad,
    mergeable,
    named,
    nameOf,
    RecordDoc,
    recordRefusalText,
    requireMayor,
    restore,
} from "../src/record.ts";

import {
    addAgent,
    addHamlet,
    context,
    fauxModels,
    ledgerOf,
    scratch,
    testHome,
} from "./support.ts";

test("admission owns realm names, the resident cap and stable restoration", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    await addHamlet(home, "other", "other-mayor");
    await home.harness.commit(async (tx) => {
        (await tx.doc(LimitsDoc, "home", "home")).mostAgents = 2;
        const options = {
            purpose: "Tests",
            model: null,
            character: "Careful",
            by: "mayor",
            reason: "Needed",
            at: 100,
        };
        expect(
            await admit(tx, "home", { ...options, name: "FelineBiologist" }),
        ).toEqual({ ok: true, value: { hamlet: "home", number: 1 } });
        expect(
            await admit(tx, "other", { ...options, name: "feline-biologist" }),
        ).toMatchObject({ ok: false, error: { kind: "name-taken" } });
        expect(await admit(tx, "home", { ...options, name: "second" })).toEqual(
            { ok: true, value: { hamlet: "home", number: 2 } },
        );
        expect(
            await admit(tx, "home", { ...options, name: "third" }),
        ).toMatchObject({ ok: false, error: { kind: "hamlet-full", most: 2 } });
        const gone = await depart(tx, "home", "feline-biologist", {
            by: "chief:chief",
            reason: "Done",
            how: "removed",
            at: 101,
        });
        expect(gone.ok).toBe(true);
        if (!gone.ok) return;
        expect(
            await restore(tx, "home", gone.value.id, {
                name: "returned",
                by: "chief:chief",
                reason: "Again",
                at: 102,
            }),
        ).toMatchObject({
            ok: true,
            value: { agent: { number: 1, name: "returned" } },
        });
        const record = await tx.doc(RecordDoc, "home", "home");
        expect(record.next).toBe(3);
        expect(nameOf(record, 1)).toBe("returned");
        expect(endName(record, "office")).toBe("mayor");
        expect(byWords("chief:chief")).toBe("the chief chief");
        expect(
            await requireMayor(tx, { hamlet: "home", number: 1 }),
        ).toMatchObject({ ok: false, error: { kind: "not-mayor" } });
    }, context);
});

test("a mayor cannot depart a protected agent or the office; refusal leaves membership intact", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    await home.harness.commit(async (tx) => {
        await admit(tx, "home", {
            name: "tester",
            purpose: "Tests",
            by: "mayor",
            reason: "Needed",
            at: 100,
        });
        const instructions = await tx.doc(
            InstructionsDoc,
            agentKey({ hamlet: "home", number: 1 }),
            agentKey({ hamlet: "home", number: 1 }),
        );
        instructions.given.push({
            chief: "chief",
            text: "Test it",
            task: "task",
            at: 100,
            done: null,
            hearing: { agent: false, mayor: false, cleared: false },
        });
        expect(
            await depart(tx, "home", "tester", {
                by: "mayor",
                reason: " ",
                how: "removed",
                at: 101,
            }),
        ).toMatchObject({ ok: false, error: { kind: "reason-empty" } });
        await admit(tx, "home", {
            name: "peer",
            purpose: "Works",
            by: "mayor",
            reason: "Needed",
            at: 100,
        });
        expect(
            await mergeable(tx, "home", {
                keep: "tester",
                absorb: "peer",
                purpose: " ",
                reason: "Done",
                by: "mayor",
                at: 101,
            }),
        ).toMatchObject({ ok: false, error: { kind: "merged-purpose-empty" } });
        const refused = await depart(tx, "home", "tester", {
            by: "mayor",
            reason: "Done",
            how: "removed",
            protectMs: 0,
            at: 101,
        });
        expect(refused).toMatchObject({
            ok: false,
            error: { kind: "protected" },
        });
        if (!refused.ok)
            expect(recordRefusalText(refused.error, "agent")).toContain(
                "No agent may be removed or merged to avoid a chief's task.",
            );
        expect(
            await depart(tx, "home", "mayor", {
                by: "chief:chief",
                reason: "Done",
                how: "removed",
                at: 101,
            }),
        ).toMatchObject({ ok: false, error: { kind: "mayor-untouchable" } });
        expect((await tx.doc(RecordDoc, "home", "home")).archive).toEqual([]);
    }, context);
});

test.each([
    { how: "removed", learned: false },
    { how: "merged", learned: false },
    { how: "merged", learned: true },
] as const)(
    "restores $how membership with learned roads $learned, uncharged and over the current cap",
    async ({ how, learned }) => {
        const home = await testHome(scratch(), {
            models: fauxModels().models,
            now: () => 1234,
        });
        await addHamlet(home, "mill", "miller");
        const archived = await home.harness.commit(async (tx) => {
            for (const name of ["scout", "smith", "poet", "scribe"])
                await admit(tx, "home", {
                    name,
                    purpose: name === "scout" ? "Maps" : "Works",
                    by: "mayor",
                    reason: "Needed",
                    at: 100,
                });
            for (const peer of ["smith", "poet", "scribe"]) {
                expect(
                    (
                        await layRoad(tx, "home", "scout", peer, {
                            by: "mayor",
                            reason: "Needed",
                            at: 101,
                        })
                    ).ok,
                ).toBe(true);
                const r = await tx.doc(RecordDoc, "home", "home");
                const number = named(r, peer)?.number;
                if (number === undefined)
                    throw new Error("Missing admitted peer");
                for (let n = 0; n < 4; n++) carryRoad(r, 1, number, 102);
            }
            const options = {
                by: "chief:chief",
                reason: "Work ended",
                at: 103,
            };
            const gone = await depart(
                tx,
                "home",
                "scout",
                how === "removed"
                    ? { ...options, how }
                    : {
                          ...options,
                          how,
                          into: 2,
                          purpose: "Both",
                          notes: "joined",
                          merge: {
                              notesBefore: "Before",
                              notesAfter: "After",
                              brought: [],
                              clashes: [],
                          },
                      },
            );
            if (!gone.ok)
                throw new Error(recordRefusalText(gone.error, "chief"));
            if (how === "merged") {
                const r = await tx.doc(RecordDoc, "home", "home");
                const entry = r.archive.find((a) => a.id === gone.value.id);
                if (entry?.merge) entry.merge.applied = true;
                if (learned) carryRoad(r, 2, 4, 104);
            }
            await depart(tx, "home", "poet", { ...options, how: "removed" });
            await admit(tx, "home", {
                name: "poet",
                purpose: "New work",
                by: "mayor",
                reason: "Needed",
                at: 105,
            });
            await admit(tx, "mill", {
                name: "scout",
                purpose: "New work",
                by: "mayor",
                reason: "Needed",
                at: 105,
            });
            (await tx.doc(LimitsDoc, "home", "home")).mostRoads = 0;
            return gone.value.id;
        }, context);
        const charged = await ledgerOf(home, "home", localDay(1234), context);
        const restored = await home.harness.commit(async (tx) => {
            const refused = await restore(tx, "home", archived, {
                name: "scout",
                by: "chief:chief",
                reason: "Mistake",
                at: 106,
            });
            expect(refused).toMatchObject({
                ok: false,
                error: { kind: "name-taken" },
            });
            if (!refused.ok)
                expect(recordRefusalText(refused.error, "chief")).toContain(
                    "as <new-name>",
                );
            return restore(tx, "home", archived, {
                name: "old-scout",
                by: "chief:chief",
                reason: "Mistake",
                at: 106,
            });
        }, context);
        expect(restored).toMatchObject({
            ok: true,
            value: { agent: { number: 1, name: "old-scout", purpose: "Maps" } },
        });
        if (!restored.ok) return;
        expect(restored.value.text).toContain(
            how === "removed" ? "Restored `old-scout`" : "Split `old-scout`",
        );
        expect(restored.value.text).toContain(
            "more than the 0 your limits allow",
        );
        const r = await home.harness.snapshot(RecordDoc, "home", context);
        expect(
            r?.roads.filter(
                (road) =>
                    (road.a === 1 || road.b === 1) &&
                    road.a !== "office" &&
                    road.b !== "office",
            ),
        ).toEqual([
            expect.objectContaining({ a: 1, b: 2, messages: 4 }),
            expect.objectContaining({ a: 1, b: 4, messages: 4 }),
        ]);
        if (how === "merged") {
            expect(r?.agents.find((a) => a.number === 2)?.purpose).toBe(
                "Works",
            );
            if (learned)
                expect(r?.roads).toContainEqual(
                    expect.objectContaining({ a: 2, b: 4, messages: 1 }),
                );
            else
                expect(
                    r?.roads.some((road) => road.a === 2 && road.b === 4),
                ).toBe(false);
        }
        expect(await ledgerOf(home, "home", localDay(1234), context)).toEqual(
            charged,
        );
    },
);

test("checks founder and restored names against the same living realm names", async () => {
    const { takenName } = await import("../src/record/names.ts");
    const { RealmDoc } = await import("../src/realm/places.ts");
    const home = await testHome(scratch(), { models: fauxModels().models });
    await addAgent(home, "home", "poet");
    expect(
        await home.harness.commit(
            async (tx) =>
                takenName(
                    await tx.doc(RealmDoc),
                    ["free", "poet"],
                    async (id) =>
                        (await tx.doc(RecordDoc, id, id)).agents.map(
                            (a) => a.name,
                        ),
                ),
            context,
        ),
    ).toBe("poet");
});
