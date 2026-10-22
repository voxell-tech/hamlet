import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { DOWN_DIR, migrationNames, sqlStatements } from "../scripts/migrationFiles.ts";
import { applyMigrations, startMiniflareD1 } from "./support/harness.ts";

type D1 = Awaited<ReturnType<typeof startMiniflareD1>>["db"];

let d1: Awaited<ReturnType<typeof startMiniflareD1>>;
beforeAll(async () => {
  d1 = await startMiniflareD1();
});
afterAll(async () => {
  await d1.dispose();
});

const APP_TABLES = [
  "Asset",
  "AssetPurchase",
  "Listing",
  "Purchase",
  "Review",
  "Session",
  "User",
  "Version",
];

async function appTables(db: D1): Promise<string[]> {
  const { results } = await db
    .prepare(
      "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_cf_%' AND name <> 'd1_migrations' ORDER BY name",
    )
    .all<{ name: string }>();
  return results.map((row) => row.name);
}

describe("migrations", () => {
  it("has at least one migration with a matching down file", () => {
    expect(migrationNames().length).toBeGreaterThan(0);
  });

  it("go all the way up, all the way down, and up again", async () => {
    expect(await appTables(d1.db)).toEqual([]);
    await applyMigrations(d1.db, "up");
    expect(await appTables(d1.db)).toEqual(APP_TABLES);

    await d1.db.batch(
      [
        "INSERT INTO User (id, githubId, login, name, avatarUrl, createdAt, updatedAt) VALUES ('u_1', '1', 'a', 'A', '', 0, 0)",
        "INSERT INTO Asset (id, target, category, title, summary, priceCents, version, fileKey, contentType, frames, status, createdAt, updatedAt) VALUES ('a-1', 'agent', 'skin', 'T', 'S', 0, '1.0.0', 'k', 'application/json', 0, 'listed', 0, 0)",
        "INSERT INTO AssetPurchase (id, assetId, buyerId, status, amountCents, createdAt, updatedAt) VALUES ('ap_1', 'a-1', 'u_1', 'paid', 0, 0, 0)",
      ].map((sql) => d1.db.prepare(sql)),
    );
    await applyMigrations(d1.db, "down");
    expect(await appTables(d1.db)).toEqual([]);

    await applyMigrations(d1.db, "up");
    expect(await appTables(d1.db)).toEqual(APP_TABLES);
  });

  it("undo add_assets alone, keeping the rows of the tables before it", async () => {
    const fresh = await startMiniflareD1();
    try {
      await applyMigrations(fresh.db, "up");
      await fresh.db
        .prepare(
          "INSERT INTO User (id, githubId, login, name, avatarUrl, createdAt, updatedAt) VALUES ('u_1', '1', 'a', 'A', '', 0, 0)",
        )
        .run();
      const down = readFileSync(join(DOWN_DIR, "0002_add_assets.sql"), "utf8");
      await fresh.db.batch(sqlStatements(down).map((statement) => fresh.db.prepare(statement)));
      expect(await appTables(fresh.db)).toEqual(
        APP_TABLES.filter((name) => !name.startsWith("Asset")),
      );
      const users = await fresh.db.prepare("SELECT count(*) AS n FROM User").first<{ n: number }>();
      expect(users!.n).toBe(1);
    } finally {
      await fresh.dispose();
    }
  });
});
