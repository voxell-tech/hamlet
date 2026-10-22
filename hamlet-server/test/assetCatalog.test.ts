import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  type CatalogEntry,
  bucketName,
  missingFiles,
  parseCatalog,
  publishSql,
  uploadsFor,
} from "../scripts/assetCatalog.ts";
import { ROOT, sqlStatements } from "../scripts/migrationFiles.ts";
import { ASSET_FIXTURE_DIR, fixtureCatalog } from "./support/assets.ts";
import { applyMigrations, startMiniflareD1 } from "./support/harness.ts";

const raw = () =>
  JSON.parse(readFileSync(join(ASSET_FIXTURE_DIR, "catalog.json"), "utf8")) as Record<
    string,
    unknown
  >[];

describe("the catalog", () => {
  it("reads the fixture, taking files/<name> or <name>", () => {
    const entries = fixtureCatalog();
    expect(entries.map((entry) => entry.id)).toEqual([
      "agent-skin-sakura",
      "hamlet-mesh-well",
      "agent-emote-wave",
    ]);
    expect(entries[1]!.file).toBe("hamlet-mesh-well.glb");
  });

  it("refuses bad entries with a readable message", () => {
    const bad = (change: Record<string, unknown>) => () =>
      parseCatalog([{ ...raw()[0], ...change }]);
    expect(bad({ id: "Sakura" })).toThrow(/\[0\]\.id/);
    expect(bad({ id: "x".repeat(65) })).toThrow(/\[0\]\.id/);
    expect(bad({ target: "plugin" })).toThrow(/\[0\]\.target/);
    expect(bad({ category: "hat" })).toThrow(/\[0\]\.category/);
    expect(bad({ priceCents: 49 })).toThrow(/\[0\]\.priceCents/);
    expect(bad({ version: "1.0" })).toThrow(/\[0\]\.version/);
    expect(bad({ frames: 73 })).toThrow(/\[0\]\.frames/);
    expect(bad({ file: "../secret.json" })).toThrow(/\[0\]\.file/);
    expect(bad({ title: "two\nlines" })).toThrow(/\[0\]\.title/);
    expect(bad({ contentType: "application/octet-stream" })).toThrow(/application\/json/);
    expect(() => parseCatalog([raw()[0], raw()[0]])).toThrow(/twice/);
    expect(() => parseCatalog({})).toThrow(/array/);
  });

  it("names every file and preview in R2 by id and version", () => {
    const uploads = uploadsFor("/out", fixtureCatalog());
    expect(uploads.map(({ key, path, contentType }) => [key, path, contentType])).toEqual([
      [
        "assets/agent-skin-sakura/1.0.0/file",
        "/out/files/agent-skin-sakura.json",
        "application/json",
      ],
      [
        "assets/agent-skin-sakura/1.0.0/preview/0.png",
        "/out/previews/agent-skin-sakura/0.png",
        "image/png",
      ],
      [
        "assets/agent-skin-sakura/1.0.0/preview/1.png",
        "/out/previews/agent-skin-sakura/1.png",
        "image/png",
      ],
      [
        "assets/hamlet-mesh-well/1.0.0/file",
        "/out/files/hamlet-mesh-well.glb",
        "model/gltf-binary",
      ],
      [
        "assets/hamlet-mesh-well/1.0.0/preview/0.png",
        "/out/previews/hamlet-mesh-well/0.png",
        "image/png",
      ],
      [
        "assets/agent-emote-wave/1.0.0/file",
        "/out/files/agent-emote-wave.json",
        "application/json",
      ],
    ]);
  });

  it("finds missing files", () => {
    expect(missingFiles(uploadsFor(ASSET_FIXTURE_DIR, fixtureCatalog()))).toEqual([]);
    const tooMany = fixtureCatalog().map((entry) => ({ ...entry, frames: entry.frames + 1 }));
    expect(missingFiles(uploadsFor("/out", tooMany), (path) => !path.endsWith("/2.png"))).toEqual([
      "/out/previews/agent-skin-sakura/2.png",
    ]);
  });

  it("finds the bucket in wrangler.jsonc", () => {
    expect(bucketName(readFileSync(join(ROOT, "wrangler.jsonc"), "utf8"))).toBe(
      "hamlet-bazaar-blobs",
    );
    expect(() => bucketName('{ "r2_buckets": [] }')).toThrow(/BLOBS/);
  });
});

type D1 = Awaited<ReturnType<typeof startMiniflareD1>>["db"];
async function publish(db: D1, entries: CatalogEntry[], at: string): Promise<void> {
  const sql = publishSql(entries, new Date(at));
  await db.batch(sqlStatements(sql).map((statement) => db.prepare(statement)));
}

async function rows(db: D1) {
  const result = await db
    .prepare('SELECT id, title, status, fileKey, createdAt, updatedAt FROM "Asset" ORDER BY id')
    .all<Record<string, string>>();
  return result.results;
}

describe("the publish SQL", () => {
  let d1: Awaited<ReturnType<typeof startMiniflareD1>>;
  beforeEach(async () => {
    d1 = await startMiniflareD1();
    await applyMigrations(d1.db, "up");
  });
  afterEach(async () => {
    await d1.dispose();
  });

  it("upserts as listed, idempotently, and unlists what left the catalog", async () => {
    const entries = fixtureCatalog();
    await publish(d1.db, entries, "2026-01-01T00:00:00Z");
    const first = await rows(d1.db);
    expect(first).toEqual([
      expect.objectContaining({ id: "agent-emote-wave", status: "listed" }),
      expect.objectContaining({
        id: "agent-skin-sakura",
        status: "listed",
        fileKey: "assets/agent-skin-sakura/1.0.0/file",
        createdAt: "2026-01-01T00:00:00.000+00:00",
        updatedAt: "2026-01-01T00:00:00.000+00:00",
      }),
      expect.objectContaining({ id: "hamlet-mesh-well", status: "listed" }),
    ]);

    await publish(d1.db, entries, "2026-02-01T00:00:00Z");
    expect(await rows(d1.db)).toEqual(first);

    const renamed = entries
      .filter((entry) => entry.id !== "agent-emote-wave")
      .map((entry) =>
        entry.id === "hamlet-mesh-well" ? { ...entry, title: "Bob's Well" } : entry,
      );
    await publish(d1.db, renamed, "2026-03-01T00:00:00Z");
    const after = await rows(d1.db);
    expect(after[0]).toMatchObject({
      id: "agent-emote-wave",
      status: "unlisted",
      updatedAt: "2026-03-01T00:00:00.000+00:00",
    });
    expect(after[1]).toEqual(first[1]);
    expect(after[2]).toMatchObject({
      title: "Bob's Well",
      createdAt: "2026-01-01T00:00:00.000+00:00",
      updatedAt: "2026-03-01T00:00:00.000+00:00",
    });

    await publish(d1.db, [], "2026-04-01T00:00:00Z");
    expect((await rows(d1.db)).map((row) => row.status)).toEqual([
      "unlisted",
      "unlisted",
      "unlisted",
    ]);
  });
});
