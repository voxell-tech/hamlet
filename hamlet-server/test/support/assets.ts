import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  type CatalogEntry,
  parseCatalog,
  publishSql,
  uploadsFor,
} from "../../scripts/assetCatalog.ts";
import { sqlStatements } from "../../scripts/migrationFiles.ts";
import type { Harness } from "./harness.ts";

export const ASSET_FIXTURE_DIR = join(import.meta.dirname, "..", "fixtures", "assets");

export function fixtureCatalog(): CatalogEntry[] {
  return parseCatalog(JSON.parse(readFileSync(join(ASSET_FIXTURE_DIR, "catalog.json"), "utf8")));
}

/** What `pnpm assets:publish` does, with the harness's D1 and R2 in place of Wrangler. */
export async function publishAssets(
  h: Harness,
  entries: CatalogEntry[],
  now = new Date(),
): Promise<void> {
  for (const upload of uploadsFor(ASSET_FIXTURE_DIR, entries)) {
    await h.services.blobs.put(upload.key, readFileSync(upload.path), {
      httpMetadata: { contentType: upload.contentType },
    });
  }
  const sql = publishSql(entries, now);
  await h.db.batch(sqlStatements(sql).map((statement) => h.db.prepare(statement)));
}
