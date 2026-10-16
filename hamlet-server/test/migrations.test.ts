import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { migrationNames } from "../scripts/migrationFiles.ts";
import { applyMigrations, startMiniflareD1 } from "./support/harness.ts";

type D1 = Awaited<ReturnType<typeof startMiniflareD1>>["db"];

let d1: Awaited<ReturnType<typeof startMiniflareD1>>;
beforeAll(async () => {
  d1 = await startMiniflareD1();
});
afterAll(async () => {
  await d1.dispose();
});

const APP_TABLES = ["Listing", "Purchase", "Review", "Session", "User", "Version"];

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

    await d1.db
      .prepare(
        "INSERT INTO User (id, githubId, login, name, avatarUrl, createdAt, updatedAt) VALUES ('u_1', '1', 'a', 'A', '', 0, 0)",
      )
      .run();
    await applyMigrations(d1.db, "down");
    expect(await appTables(d1.db)).toEqual([]);

    await applyMigrations(d1.db, "up");
    expect(await appTables(d1.db)).toEqual(APP_TABLES);
  });
});
