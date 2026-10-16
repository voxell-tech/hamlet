import { randomUUID } from "node:crypto";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, it } from "vitest";
import { isChief, RealmDoc } from "../src/realm.ts";
import { say } from "../src/say.ts";
import {
    addHamlet,
    context,
    fauxModels,
    scratch,
    testHome,
} from "./support.ts";

it("keeps a single-user home's chief authority when its environment name changes", async () => {
    const dir = scratch(),
        models = fauxModels().models;
    const home = await testHome(dir, { models, chief: "alice" });
    expect(
        (await home.harness.snapshot(RealmDoc, context))?.hamlets[0]?.chiefs,
    ).toEqual([]);
    await home.close(context);
    for (const env of [{ HAMLET_USER: "bob" }, { USER: "carol" }, {}]) {
        const again = await testHome(dir, { models, chief: "", env });
        expect(
            await say(
                again,
                {
                    type: "say",
                    id: randomUUID(),
                    to: { hamlet: "home", agent: "mayor" },
                    text: "/budget",
                },
                context,
            ),
        ).toMatchObject({ ok: true });
        await again.close(context);
    }
});

it("migrates the first opener's stored chief without changing another hamlet's explicit chiefs", async () => {
    const dir = scratch(),
        models = fauxModels().models;
    const home = await testHome(dir, { models, chief: "alice" });
    await addHamlet(home, "mill", "miller");
    const realm = structuredClone(
        await home.harness.snapshot(RealmDoc, context),
    );
    if (!realm?.hamlets[0]) throw new Error("no home");
    realm.hamlets[0].chiefs = ["alice"];
    await home.close(context);
    const db = new DatabaseSync(join(dir, "home.sqlite"));
    db.prepare(
        "UPDATE document_revisions SET kind = 'base', version = 1, content = ? WHERE document_id IN (SELECT id FROM documents WHERE kind = ?) AND seq = (SELECT MAX(seq) FROM document_revisions WHERE document_id IN (SELECT id FROM documents WHERE kind = ?))",
    ).run(
        JSON.stringify(realm),
        JSON.stringify("hamlet.realm"),
        JSON.stringify("hamlet.realm"),
    );
    db.close();
    const again = await testHome(dir, { models, chief: "bob" });
    expect(
        await say(
            again,
            {
                type: "say",
                id: randomUUID(),
                to: { hamlet: "home", agent: "mayor" },
                text: "/new",
            },
            context,
        ),
    ).toMatchObject({ ok: true });
    const migrated = await again.harness.snapshot(RealmDoc, context);
    if (!migrated) throw new Error("no realm");
    expect(isChief(migrated, "home", "bob")).toBe(true);
    expect(isChief(migrated, "mill", "bob")).toBe(false);
    expect(isChief(migrated, "mill", "alice")).toBe(true);
});
