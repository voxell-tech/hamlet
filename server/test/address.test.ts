import { expect, it } from "vitest";
import { livingAgent, resolveAddress } from "../src/realm.ts";
import { RecordDoc } from "../src/record.ts";
import { addAgent, context, fauxModels, scratch, testHome } from "./support.ts";

it("resolves the office at use time and classifies missing and departed agents", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const { number } = await addAgent(home, "home", "baker");
    const to = { hamlet: "home", agent: "mayor" };
    expect(await livingAgent(home, to, "stop", context)).toMatchObject({
        ok: true,
        value: {
            ref: { hamlet: "home", number: 0 },
            chat: { chief: home.chief },
        },
    });
    await home.harness.commit(async (tx) => {
        (await tx.doc(RecordDoc, "home", "home")).mayor = number;
    }, context);
    expect(await livingAgent(home, to, "stop", context)).toMatchObject({
        ok: true,
        value: { ref: { hamlet: "home", number } },
    });
    expect(
        await livingAgent(
            home,
            { hamlet: "home", agent: "absent" },
            "stop",
            context,
        ),
    ).toEqual({
        ok: false,
        error: {
            kind: "agent-missing",
            operation: "stop",
            hamlet: "home",
            agent: "absent",
            retrySafe: true,
        },
    });
    await home.harness.commit(async (tx) => {
        const record = await tx.doc(RecordDoc, "home", "home");
        const agent = record.agents.find((a) => a.name === "baker");
        if (!agent) throw new Error("No baker");
        record.agents = record.agents.filter((a) => a !== agent);
        record.archive.push({
            id: "removed",
            into: null,
            merge: null,
            agent,
            how: "removed",
            by: "chief:chief",
            reason: "Finished",
            at: 1,
            back: null,
            roads: [],
        });
    }, context);
    expect(
        await livingAgent(
            home,
            { hamlet: "home", agent: "baker" },
            "stop",
            context,
        ),
    ).toMatchObject({
        ok: false,
        error: {
            kind: "agent-gone",
            operation: "stop",
            retrySafe: false,
            farewell: expect.stringContaining("was removed"),
        },
    });
    expect(
        await resolveAddress(home.harness, "home", "baker", context),
    ).toMatchObject({ ok: false, error: { kind: "departed" } });
    expect(
        await resolveAddress(home.harness, "home", "baker@home@extra", context),
    ).toMatchObject({ ok: false, error: { kind: "malformed" } });
});
