/**
 * Database migrations, up and down, for D1.
 *
 *   pnpm db:new <name>   write migrations/NNNN_<name>.sql and its undo
 *   pnpm db:up           apply pending migrations (local; add --remote)
 *   pnpm db:down         undo the newest applied migration (local; add --remote)
 *   pnpm db:status       list applied and pending migrations
 *
 * Wrangler and Prisma only go up. The undo of each migration is the
 * reverse Prisma diff, kept in migrations-down/ under the same name. Both
 * are rewritten by d1Sql.ts so they run on D1 without losing rows.
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  DOWN_DIR,
  ROOT,
  SCHEMA_PATH,
  SNAPSHOT_DIR,
  UP_DIR,
  migrationNames,
} from "./migrationFiles.ts";
import { adaptForD1, tablesAfter } from "./d1Sql.ts";

const DATABASE = "DB";
const MIGRATIONS_TABLE = "d1_migrations";
const NAME_PATTERN = /^[a-z0-9_]+$/;

function main(): void {
  const [command, ...args] = process.argv.slice(2);
  switch (command) {
    case "new":
      return newMigration(args[0]);
    case "up":
      return up(args);
    case "down":
      return down(args);
    case "status":
      return status(args);
    default:
      fail("usage: db.ts new <name> | up | down | status [--remote]");
  }
}

function newMigration(name: string | undefined): void {
  if (!name || !NAME_PATTERN.test(name)) {
    fail("give the migration a name of lowercase letters, digits and _, e.g. add_bundles");
  }
  const existing = migrationNames();
  const previous = existing.at(-1)?.replace(/\.sql$/, "");
  const previousSchema = previous ? join(SNAPSHOT_DIR, `${previous}.prisma`) : null;
  if (previousSchema && !existsSync(previousSchema)) {
    fail(`missing snapshot ${previousSchema}; it is needed to diff against`);
  }
  const fromPrevious = previousSchema ? ["--schema", previousSchema] : ["--empty"];
  const upSql = prismaDiff(fromPrevious, ["--schema", SCHEMA_PATH]);
  if (isEmptyDiff(upSql)) fail("schema.prisma has not changed since the last migration");
  const downSql = prismaDiff(["--schema", SCHEMA_PATH], fromPrevious);

  const number = String(existing.length === 0 ? 1 : Number(previous!.slice(0, 4)) + 1).padStart(
    4,
    "0",
  );
  const fileName = `${number}_${name}`;
  const earlier = existing.map(readUp);
  const up = adaptForD1(upSql, tablesAfter(earlier));
  const down = adaptForD1(downSql, tablesAfter([...earlier, up]));
  writeFileSync(join(UP_DIR, `${fileName}.sql`), up);
  writeFileSync(join(DOWN_DIR, `${fileName}.sql`), down);
  copyFileSync(SCHEMA_PATH, join(SNAPSHOT_DIR, `${fileName}.prisma`));
  console.log(`wrote migrations/${fileName}.sql, migrations-down/${fileName}.sql`);
  console.log(`and prisma/snapshots/${fileName}.prisma. Read both SQL files before applying.`);
  if (`${up}${down}`.includes("Set aside")) {
    console.log(
      "It rebuilds a table other tables point at: their rows are set aside and put back.",
    );
  }
}

function up(args: string[]): void {
  run("wrangler", ["d1", "migrations", "apply", DATABASE, ...target(args)]);
}

function down(args: string[]): void {
  const newest = appliedMigrations(args).at(-1);
  if (!newest) fail("no migration is applied");
  const downFile = join(DOWN_DIR, newest);
  if (!existsSync(downFile)) fail(`missing ${downFile}`);
  console.log(`undoing ${newest}`);
  run("wrangler", ["d1", "execute", DATABASE, ...target(args), "--yes", "--file", downFile]);
  const escaped = newest.replaceAll("'", "''");
  run("wrangler", [
    "d1",
    "execute",
    DATABASE,
    ...target(args),
    "--yes",
    "--command",
    `DELETE FROM ${MIGRATIONS_TABLE} WHERE name = '${escaped}'`,
  ]);
}

function status(args: string[]): void {
  const applied = appliedMigrations(args);
  for (const name of migrationNames()) {
    console.log(`${applied.includes(name) ? "applied" : "pending"}  ${name}`);
  }
}

/** Names in wrangler's migrations table, oldest first. */
function appliedMigrations(args: string[]): string[] {
  const result = capture("wrangler", [
    "d1",
    "execute",
    DATABASE,
    ...target(args),
    "--json",
    "--command",
    `SELECT name FROM sqlite_master WHERE type = 'table' AND name = '${MIGRATIONS_TABLE}'`,
  ]);
  if (firstResults(result).length === 0) return [];
  const rows = firstResults(
    capture("wrangler", [
      "d1",
      "execute",
      DATABASE,
      ...target(args),
      "--json",
      "--command",
      `SELECT name FROM ${MIGRATIONS_TABLE} ORDER BY id`,
    ]),
  );
  return rows.map((row) => String(row.name));
}

function firstResults(json: string): Record<string, unknown>[] {
  const parsed = JSON.parse(json) as { results?: Record<string, unknown>[] }[];
  return parsed[0]?.results ?? [];
}

/** `--local` unless `--remote` (or `--preview`) is asked for. */
function target(args: string[]): string[] {
  const remote = args.includes("--remote") || args.includes("--preview");
  return remote ? args : ["--local", ...args];
}

function prismaDiff(from: string[], to: string[]): string {
  const [fromFlag, ...fromRest] = from;
  const [toFlag, ...toRest] = to;
  return capture("prisma", [
    "migrate",
    "diff",
    `--from-${fromFlag!.slice(2)}`,
    ...fromRest,
    `--to-${toFlag!.slice(2)}`,
    ...toRest,
    "--script",
  ]);
}

function isEmptyDiff(sql: string): boolean {
  return sql.split("\n").every((line) => line.trim() === "" || line.trim().startsWith("--"));
}

function readUp(name: string): string {
  return readFileSync(join(UP_DIR, name), "utf8");
}

function bin(name: string): string {
  return join(ROOT, "node_modules", ".bin", name);
}

function run(name: string, args: string[]): void {
  const result = spawnSync(bin(name), args, { cwd: ROOT, stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function capture(name: string, args: string[]): string {
  const result = spawnSync(bin(name), args, {
    cwd: ROOT,
    encoding: "utf8",
    env: { ...process.env, PRISMA_HIDE_UPDATE_MESSAGE: "1" },
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.status !== 0) {
    process.stderr.write(result.stderr);
    process.stderr.write(result.stdout);
    process.exit(result.status ?? 1);
  }
  return result.stdout;
}

function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

main();
