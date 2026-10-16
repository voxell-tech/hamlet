import { readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
/** Applied by `wrangler d1 migrations apply` (wrangler.jsonc `migrations_dir`). */
export const UP_DIR = join(ROOT, "migrations");
/** The matching undo of each migration, kept out of wrangler's sight. */
export const DOWN_DIR = join(ROOT, "migrations-down");
/** The schema as it was after each migration, to diff the next one against. */
export const SNAPSHOT_DIR = join(ROOT, "prisma", "snapshots");
export const SCHEMA_PATH = join(ROOT, "prisma", "schema.prisma");

const MIGRATION_FILE = /^(\d{4})_[a-z0-9_]+\.sql$/;

/** Migration file names (`0001_init.sql`, ...), oldest first. */
export function migrationNames(): string[] {
  return readdirSync(UP_DIR)
    .filter((name) => MIGRATION_FILE.test(name))
    .sort();
}

/**
 * Splits a migration into statements, as D1's batch API runs one at a
 * time. Comment lines are dropped unless `keepComments`, which keeps each
 * with the statement after it.
 */
export function sqlStatements(sql: string, keepComments = false): string[] {
  const statements: string[] = [];
  let current: string[] = [];
  for (const line of sql.split("\n")) {
    const trimmed = line.trim();
    if (trimmed === "") continue;
    if (trimmed.startsWith("--") && !keepComments) continue;
    current.push(line);
    if (!trimmed.startsWith("--") && trimmed.endsWith(";")) {
      statements.push(current.join("\n"));
      current = [];
    }
  }
  if (current.length > 0) statements.push(current.join("\n"));
  return statements;
}
