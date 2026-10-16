import { expect, test } from "vitest";
import { InstructionsDoc } from "../src/chats/instructions.ts";
import { LimitsDoc } from "../src/limits.ts";
import {
    admit,
    agentKey,
    byWords,
    depart,
    endName,
    nameOf,
    RecordDoc,
    recordRefusalText,
    requireMayor,
    restore,
} from "../src/record.ts";
import {
    addHamlet,
    context,
    fauxModels,
    scratch,
    testHome,
} from "./support.ts";

test("admission owns realm names, the total membership cap and stable restoration", async () => {
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
        expect(record.next).toBe(2);
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
