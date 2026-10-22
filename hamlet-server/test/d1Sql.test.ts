import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { adaptForD1, tablesAfter } from "../scripts/d1Sql.ts";
import { UP_DIR, migrationNames, sqlStatements } from "../scripts/migrationFiles.ts";
import { applyMigrations, startMiniflareD1 } from "./support/harness.ts";

// Raw `prisma migrate diff` output: from the first schema to one with a
// column inserted in Listing (rebuilt) plus a Bundle table, and back; and
// one that rebuilds User, which every other table hangs off.
const fixture = (name: string) => readFileSync(join(import.meta.dirname, "fixtures", name), "utf8");
const initialSql = () => migrationNames().map((name) => readFileSync(join(UP_DIR, name), "utf8"));

type D1 = Awaited<ReturnType<typeof startMiniflareD1>>["db"];
let d1: Awaited<ReturnType<typeof startMiniflareD1>>;
beforeEach(async () => {
  d1 = await startMiniflareD1();
  await applyMigrations(d1.db, "up");
  await seed(d1.db);
});
afterEach(async () => {
  await d1.dispose();
});

async function seed(db: D1): Promise<void> {
  await db.batch(
    [
      "INSERT INTO User (id, githubId, login, name, avatarUrl, createdAt, updatedAt) VALUES ('u1', '1', 'a', 'A', '', 0, 0), ('u2', '2', 'b', 'B', '', 0, 0)",
      "INSERT INTO Session (id, tokenHash, userId, expiresAt, createdAt) VALUES ('s1', 'h', 'u1', 0, 0)",
      "INSERT INTO Listing (id, kind, title, summary, description, category, tags, priceCents, status, sellerId, createdAt, updatedAt) VALUES ('l1', 'agent', 't', 's', 'd', 'c', '[]', 0, 'listed', 'u1', 0, 0)",
      "INSERT INTO Version (id, listingId, version, major, minor, patch, changes, blobKey, size, sha256, meta, createdAt) VALUES ('v1', 'l1', '1.0.0', 1, 0, 0, '', 'k', 1, 'h', '{}', 0)",
      "INSERT INTO Purchase (id, listingId, buyerId, status, amountCents, feeCents, createdAt, updatedAt) VALUES ('p1', 'l1', 'u2', 'paid', 0, 0, 0, 0)",
      "INSERT INTO Review (id, listingId, userId, stars, words, createdAt, updatedAt) VALUES ('r1', 'l1', 'u2', 5, 'ok', 0, 0)",
    ].map((sql) => db.prepare(sql)),
  );
}

async function run(db: D1, sql: string): Promise<void> {
  await db.batch(sqlStatements(sql).map((statement) => db.prepare(statement)));
}

async function counts(db: D1): Promise<Record<string, number>> {
  const result: Record<string, number> = {};
  for (const table of ["User", "Session", "Listing", "Version", "Purchase", "Review"]) {
    const row = await db.prepare(`SELECT count(*) AS n FROM "${table}"`).first<{ n: number }>();
    result[table] = row!.n;
  }
  return result;
}

const SEEDED = { User: 2, Session: 1, Listing: 1, Version: 1, Purchase: 1, Review: 1 };

describe("adaptForD1", () => {
  it("shows why it exists: Prisma's own rebuild loses rows on D1", async () => {
    const raw = fixture("redefine_up.sql").replace(/^PRAGMA foreign_keys=(on|off|ON|OFF);$/gm, "");
    await run(d1.db, raw);
    expect((await counts(d1.db)).Version).toBe(0);
  });

  it("rebuilds a parent table and back without losing a row", async () => {
    const up = adaptForD1(fixture("redefine_up.sql"), tablesAfter(initialSql()));
    const down = adaptForD1(fixture("redefine_down.sql"), tablesAfter([...initialSql(), up]));
    expect(up).toContain('CREATE TABLE "_d1_backup_Version" AS SELECT * FROM "Version";');
    expect(up).not.toMatch(/PRAGMA foreign_keys/i);

    await run(d1.db, up);
    expect(await counts(d1.db)).toEqual(SEEDED);
    expect(await columns(d1.db, "Listing")).toContain("featured");

    await run(d1.db, down);
    expect(await counts(d1.db)).toEqual(SEEDED);
    expect(await columns(d1.db, "Listing")).not.toContain("featured");
    const leftovers = await d1.db
      .prepare(
        "SELECT name FROM sqlite_master WHERE name LIKE '%backup%' OR name IN ('Bundle', 'new_Listing')",
      )
      .all();
    expect(leftovers.results).toEqual([]);
  });

  it("sets aside every table below a rebuilt root, nearest first", async () => {
    const up = adaptForD1(fixture("redefine_user_up.sql"), tablesAfter(initialSql()));
    const backups = [...up.matchAll(/CREATE TABLE "_d1_backup_(\w+)"/g)].map((m) => m[1]);
    expect(backups.slice(0, 5).sort()).toEqual([
      "AssetPurchase",
      "Listing",
      "Purchase",
      "Review",
      "Session",
    ]);
    expect(backups[5]).toBe("Version");
    await run(d1.db, up);
    expect(await counts(d1.db)).toEqual(SEEDED);
  });
});

async function columns(db: D1, table: string): Promise<string[]> {
  const result = await db
    .prepare(`SELECT name FROM pragma_table_info('${table}')`)
    .all<{ name: string }>();
  return result.results.map((column) => column.name);
}
