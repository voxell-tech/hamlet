import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

import { describe, expect, it } from "vitest";
import { editLimits, LimitsDoc } from "../src/limits.ts";
import { HOME } from "../src/record.ts";
import { context, fauxModels, scratch, testHome } from "./support.ts";

describe("a chief's limits", () => {
    it("keeps omitted defaults and refuses an invalid edit without changing anything", async () => {
        const home = await testHome(scratch(), { models: fauxModels().models });
        await editLimits(
            home,
            HOME,
            { whole: { task: { messages: 3 } } },
            context,
        );
        expect(
            await home.harness.snapshot(LimitsDoc, HOME, context),
        ).toMatchObject({
            budget: { cost: 5 },
            task: { cost: 1, messages: 3 },
            road: { cost: 0.01 },
        });
        for (const [key, value, kind] of [
            ["cost", -1, "invalid-field"],
            ["cost", "no", "invalid-field"],
            ["mystery", 1, "unknown-field"],
        ] as const) {
            await expect(
                editLimits(
                    home,
                    HOME,
                    { set: { section: "budget", key, value } },
                    context,
                ),
            ).resolves.toMatchObject({
                ok: false,
                error: { kind, path: `budget.${key}` },
            });
            expect(
                await home.harness.snapshot(LimitsDoc, HOME, context),
            ).toMatchObject({ budget: { cost: 5 }, task: { messages: 3 } });
        }
        await editLimits(
            home,
            HOME,
            { set: { section: "budget", key: "cost", value: null } },
            context,
        );
        expect(
            await home.harness.snapshot(LimitsDoc, HOME, context),
        ).toMatchObject({ budget: { cost: null } });
    });
});

it("fills votes and hamlets, edits both, and refuses invalid thresholds atomically", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const defaults = {
        votes: {
            passYes: 2,
            passOf: 3,
            leastVoters: 3,
            sealed: true,
            groundedOnly: false,
            ballotMinutes: 5,
            openMinutes: 60,
            retryMinutes: 60,
        },
        hamlets: { most: 6, roadsBetween: true, removalMayors: 3 },
    };
    expect(await home.harness.snapshot(LimitsDoc, HOME, context)).toMatchObject(
        defaults,
    );
    await editLimits(
        home,
        HOME,
        { whole: { votes: { sealed: false }, hamlets: { most: 4 } } },
        context,
    );
    await editLimits(
        home,
        HOME,
        { set: { section: "votes", key: "passOf", value: 2 } },
        context,
    );
    const before = await home.harness.snapshot(LimitsDoc, HOME, context);
    expect(before).toMatchObject({
        votes: { sealed: false, passOf: 2 },
        hamlets: { most: 4 },
    });
    for (const [section, key, value] of [
        ["votes", "passYes", 3],
        ["votes", "passOf", 0],
        ["votes", "ballotMinutes", -1],
        ["hamlets", "most", 1.5],
    ] as const) {
        await expect(
            editLimits(home, HOME, { set: { section, key, value } }, context),
        ).resolves.toMatchObject({ ok: false });
        expect(await home.harness.snapshot(LimitsDoc, HOME, context)).toEqual(
            before,
        );
    }
});

it("opens a stored step-2 limits document with votes and hamlets added", async () => {
    const dir = scratch();
    const models = fauxModels().models;
    const home = await testHome(dir, { models });
    await editLimits(home, HOME, { whole: { budget: { cost: 7 } } }, context);
    const old = await home.harness.snapshot(LimitsDoc, HOME, context);
    await home.close(context);
    if (old === undefined) throw new Error("no limits");
    const { votes: _votes, hamlets: _hamlets, ...step2 } = old;
    const db = new DatabaseSync(join(dir, "home.sqlite"));
    const row = db
        .prepare("SELECT id FROM documents WHERE kind = ?")
        .get(JSON.stringify("hamlet.limits"));
    if (row === undefined) throw new Error("no limits record");
    db.prepare("DELETE FROM document_revisions WHERE document_id = ?").run(
        row.id as number,
    );
    db.prepare(
        "INSERT INTO document_revisions (document_id, seq, kind, version, content) VALUES (?, 1, 'base', 1, ?)",
    ).run(row.id as number, JSON.stringify(step2));
    db.close();
    const again = await testHome(dir, { models });
    expect(
        await again.harness.snapshot(LimitsDoc, HOME, context),
    ).toMatchObject({
        budget: { cost: 7 },
        votes: { passYes: 2, passOf: 3 },
        hamlets: { most: 6 },
    });
});

it("normalizes omitted limits without mutating input and round-trips the resulting document", async () => {
    const { parseLimits } = await import("../src/limits.ts");
    const input = {
        votes: { passOf: 2 },
        budget: { cost: 0.25 },
        pinned: { mayor: "local::tiny", ["__proto__"]: "unused::name" },
    };
    const parsed = parseLimits(input);
    expect(parsed).toMatchObject({
        ok: true,
        value: {
            votes: { passYes: 2, passOf: 2 },
            budget: { cost: 0.25 },
            task: { messages: 12 },
            pinned: { mayor: "local::tiny", ["__proto__"]: "unused::name" },
        },
    });
    if (!parsed.ok) throw new Error("Valid limits were refused");
    expect(parsed.value.pinned).toEqual(input.pinned);
    expect(parseLimits(JSON.parse(JSON.stringify(parsed.value)))).toEqual(
        parsed,
    );
    expect(input).toEqual({
        votes: { passOf: 2 },
        budget: { cost: 0.25 },
        pinned: { mayor: "local::tiny", ["__proto__"]: "unused::name" },
    });
});
