import { describe, expect, it } from "vitest";
import { HOME } from "../src/hamlet.ts";
import { editLimits } from "../src/limits/edit.ts";
import { LimitsDoc } from "../src/limits.ts";
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
        for (const [key, value, words] of [
            ["cost", -1, "budget.cost: a cost is a number of zero or more"],
            ["cost", "no", "budget.cost"],
            ["mystery", 1, "unknown field `mystery`"],
        ] as const) {
            await expect(
                editLimits(
                    home,
                    HOME,
                    { set: { section: "budget", key, value } },
                    context,
                ),
            ).rejects.toThrow(words);
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
