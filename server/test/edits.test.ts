import { randomUUID } from "node:crypto";
import { expect, it } from "vitest";
import { scriptedModels } from "../src/models.ts";
import { changes, request, served } from "./connection-support.ts";

it("edits one limits key or the whole document, sends the new budget, and refuses invalid limits without a change (H22)", async () => {
    const run = await served({ models: scriptedModels({ responses: [] }) });
    const edit = async (
        body:
            | { set: { section: string; key: string; value: unknown } }
            | { whole: Record<string, unknown> },
    ) =>
        request(run.connection, run.messages, {
            type: "edit",
            id: randomUUID(),
            doc: "limits",
            hamlet: "home",
            ...body,
        });
    expect(
        await edit({ set: { section: "budget", key: "cost", value: 2 } }),
    ).toMatchObject({ type: "reply", kind: "notice" });
    expect(changes(run.messages)).toContainEqual(
        expect.objectContaining({
            type: "hamlet",
            hamlet: expect.objectContaining({
                limits: expect.objectContaining({
                    budget: expect.objectContaining({ cost: 2 }),
                }),
                budget: expect.objectContaining({
                    caps: { micros: 2_000_000, ms: null },
                    summary: "Hamlet today: $0.00 of $2.00",
                }),
            }),
        }),
    );
    expect(
        await edit({ whole: { budget: { cost: 3, minutes: 10 } } }),
    ).toMatchObject({ type: "reply", kind: "notice" });
    expect(
        changes(run.messages)
            .filter((item) => item.type === "hamlet")
            .at(-1),
    ).toMatchObject({
        hamlet: {
            limits: { budget: { cost: 3, minutes: 10 }, task: { cost: 1 } },
            budget: { caps: { micros: 3_000_000, ms: 600_000 } },
        },
    });
    const count = changes(run.messages).length;
    expect(
        await edit({ set: { section: "budget", key: "cost", value: -1 } }),
    ).toMatchObject({
        type: "reply",
        kind: "error",
        text: "budget.cost: a cost is a number of zero or more",
    });
    expect(changes(run.messages)).toHaveLength(count);
    expect(
        await edit({ set: { section: "budget", key: "minutes", value: null } }),
    ).toMatchObject({ type: "reply", kind: "notice" });
    expect(
        changes(run.messages)
            .filter((item) => item.type === "hamlet")
            .at(-1),
    ).toMatchObject({ hamlet: { limits: { budget: { minutes: null } } } });
    expect(
        await edit({ set: { section: "mayor", key: "toldDays", value: 2 } }),
    ).toMatchObject({ kind: "notice" });
    expect(
        await edit({ set: { section: "mayor", key: "toldDays", value: null } }),
    ).toMatchObject({ kind: "notice" });
    expect(
        changes(run.messages)
            .filter((item) => item.type === "hamlet")
            .at(-1),
    ).toMatchObject({ hamlet: { limits: { mayor: { toldDays: 7 } } } });
});
