import { expect, it } from "vitest";
import { defaultLimits } from "../src/limits.ts";
import { report } from "../src/spend.ts";

it("shows the meter nearest its cap, the model and a nonzero small cost", () => {
    const limits = defaultLimits();
    limits.budget.minutes = 10;
    const books = {
        hamlet: { tokens: 200_000, micros: 300_000, ms: 540_000 },
        task: { tokens: 0, micros: 0, ms: 0 },
        agents: new Map([
            ["home:1", { tokens: 12_000, micros: 300_000, ms: 0 }],
        ]),
    };
    expect(
        report(
            limits,
            books,
            [
                { key: "home:0", name: "mayor", model: "big" },
                { key: "home:1", name: "scout", model: "small" },
            ],
            new Date(2026, 5, 4, 12).getTime(),
        ),
    ).toMatchObject({
        summary: "Hamlet today: 9m model time of 10m model time",
        lines: ["- mayor: big, $0.00", "- scout: small, $0.30"],
        remains: "1m model time",
        gone: 0.5,
    });
});
