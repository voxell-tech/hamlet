import { describe, expect, it } from "vitest";
import { GrantsDoc, give, grantsOf, holds, revoke } from "../src/grants.ts";
import { context, fauxModels, scratch, testHome } from "./support.ts";

const SCOUT = { hamlet: "home", number: 7 };

describe("lasting permissions (C9, H11)", () => {
    it("keeps a grant after reopening until it is taken back, and records no second revocation (grants.rs:171)", async () => {
        const dir = scratch();
        const { models } = fauxModels();
        const first = await testHome(dir, { models });
        await first.harness.commit(
            (tx) => give(tx, SCOUT, "scout", "network", "alice", 100),
            context,
        );
        await first.close(context);
        const home = await testHome(dir, { models });
        expect(
            grantsOf(await home.harness.snapshot(GrantsDoc, context)),
        ).toEqual([
            { agent: SCOUT, name: "scout", permission: "network", by: "alice" },
        ]);
        expect(
            await home.harness.commit(
                (tx) => revoke(tx, SCOUT, "network", "bea", 200),
                context,
            ),
        ).toBe(true);
        expect(
            await home.harness.commit(
                (tx) => revoke(tx, SCOUT, "network", "bea", 300),
                context,
            ),
        ).toBe(false);
        expect(
            (await home.harness.snapshot(GrantsDoc, context))?.events,
        ).toEqual([
            {
                type: "granted",
                at: 100,
                agent: SCOUT,
                name: "scout",
                permission: "network",
                by: "alice",
            },
            {
                type: "revoked",
                at: 200,
                agent: SCOUT,
                permission: "network",
                by: "bea",
            },
        ]);
        await home.close(context);
        const again = await testHome(dir, { models });
        expect(
            grantsOf(await again.harness.snapshot(GrantsDoc, context)),
        ).toEqual([]);
    });

    it("gives a later agent of the same name nothing, and gives nothing to the same number in another hamlet (grants.rs:194)", async () => {
        const { models } = fauxModels();
        const home = await testHome(scratch(), { models });
        await home.harness.commit(async (tx) => {
            await give(tx, SCOUT, "scout", "network", "alice", 100);
            await give(tx, SCOUT, "scout", "network", "bea", 200);
            await give(tx, SCOUT, "scout", "unconfined", "bea", 300);
        }, context);
        const granted = grantsOf(
            await home.harness.snapshot(GrantsDoc, context),
        );
        expect(granted).toEqual([
            { agent: SCOUT, name: "scout", permission: "network", by: "bea" },
            {
                agent: SCOUT,
                name: "scout",
                permission: "unconfined",
                by: "bea",
            },
        ]);
        const none = { network: [], unconfined: [] };
        expect(holds(none, granted, "scout", SCOUT, "network")).toBe(true);
        expect(
            holds(none, granted, "scout", { ...SCOUT, number: 9 }, "network"),
        ).toBe(false);
        expect(
            holds(
                none,
                granted,
                "scout",
                { ...SCOUT, hamlet: "far" },
                "network",
            ),
        ).toBe(false);
        expect(
            holds(
                { ...none, network: ["scout"] },
                granted,
                "scout",
                { ...SCOUT, number: 9 },
                "network",
            ),
        ).toBe(true);
    });
});
