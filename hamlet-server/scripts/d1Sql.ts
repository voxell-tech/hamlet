/**
 * Makes Prisma's SQLite migrations safe to run on D1.
 *
 * D1 always enforces foreign keys and ignores `PRAGMA foreign_keys = OFF`,
 * which Prisma's SQLite scripts rely on. Two of its patterns then break:
 *
 * - Dropping tables in the wrong order: once a parent table is gone, SQLite
 *   cannot prepare the implicit delete that dropping its child does.
 * - Rebuilding a table ("RedefineTables": create new_T, copy, drop T,
 *   rename): dropping T deletes its rows first, which cascades into every
 *   table pointing at it, or fails the migration without a cascade.
 *
 * So drops are put children first, and before a rebuilt table is dropped
 * the rows of every table that points at it (directly or not) are set
 * aside, then put back once the new table has its name.
 */
import { sqlStatements } from "./migrationFiles.ts";

/** Each table and the tables its foreign keys point at. */
export type Tables = Map<string, Set<string>>;

const CREATE_TABLE = /^CREATE TABLE "([^"]+)" \(([\s\S]*)\)$/;
const CREATE_TABLE_AS = /^CREATE TABLE "([^"]+)" AS /;
const DROP_TABLE = /^DROP TABLE "([^"]+)"$/;
const RENAME_TABLE = /^ALTER TABLE "([^"]+)" RENAME TO "([^"]+)"$/;
const REFERENCES = /REFERENCES "([^"]+)"/g;
const FOREIGN_KEYS_PRAGMA = /^PRAGMA (defer_)?foreign_keys\s*=\s*(ON|OFF);$/i;
const BACKUP_PREFIX = "_d1_backup_";

/** The tables that exist after running these migrations in order. */
export function tablesAfter(sqls: string[]): Tables {
  const tables: Tables = new Map();
  for (const sql of sqls) {
    for (const statement of sqlStatements(sql)) track(tables, code(statement));
  }
  return tables;
}

/** Rewrites one Prisma migration script to run on D1 without losing rows. */
export function adaptForD1(sql: string, tablesBefore: Tables): string {
  const statements = sqlStatements(withoutForeignKeyPragmas(sql), true);
  const tables: Tables = new Map([...tablesBefore].map(([name, refs]) => [name, new Set(refs)]));
  const restores = new Map<string, string[]>();
  const out: string[] = [];

  for (let index = 0; index < statements.length; index++) {
    const statement = statements[index]!;
    const dropped = DROP_TABLE.exec(code(statement))?.[1];
    if (dropped && !isRebuild(tables, dropped)) {
      const run = plainDropRun(statements, index, tables);
      for (const table of childrenFirst(run, tables)) {
        out.push(`DROP TABLE "${table}";`);
        tables.delete(table);
      }
      index += run.length - 1;
      continue;
    }
    if (dropped) {
      const dependents = dependentsOf(tables, dropped);
      if (dependents.length > 0) {
        out.push(...setAside(dependents));
        restores.set(`new_${dropped}`, dependents);
      }
    }
    out.push(statement);
    track(tables, code(statement));
    const renamedFrom = RENAME_TABLE.exec(code(statement))?.[1];
    const dependents = renamedFrom ? restores.get(renamedFrom) : undefined;
    if (dependents) {
      out.push(...putBack(dependents));
      restores.delete(renamedFrom!);
    }
  }
  return `PRAGMA defer_foreign_keys = ON;\n\n${out.join("\n\n")}\n`;
}

function withoutForeignKeyPragmas(sql: string): string {
  return sql
    .split("\n")
    .filter((line) => !FOREIGN_KEYS_PRAGMA.test(line.trim()))
    .join("\n");
}

/** The statement without comment lines or the closing semicolon. */
function code(statement: string): string {
  return statement
    .split("\n")
    .filter((line) => !line.trim().startsWith("--"))
    .join("\n")
    .trim()
    .replace(/;$/, "");
}

function track(tables: Tables, statement: string): void {
  const created = CREATE_TABLE.exec(statement);
  if (created) {
    const refs = [...created[2]!.matchAll(REFERENCES)].map((match) => match[1]!);
    tables.set(created[1]!, new Set(refs.filter((name) => name !== created[1])));
    return;
  }
  const createdAs = CREATE_TABLE_AS.exec(statement);
  if (createdAs) {
    tables.set(createdAs[1]!, new Set());
    return;
  }
  const dropped = DROP_TABLE.exec(statement);
  if (dropped) {
    tables.delete(dropped[1]!);
    return;
  }
  const renamed = RENAME_TABLE.exec(statement);
  if (renamed) {
    const from = renamed[1]!;
    const to = renamed[2]!;
    const refs = tables.get(from) ?? new Set<string>();
    tables.delete(from);
    tables.set(to, refs);
  }
}

/** A drop is part of a rebuild when `new_<table>` is waiting to replace it. */
function isRebuild(tables: Tables, table: string): boolean {
  return tables.has(`new_${table}`);
}

/** The consecutive plain drops starting at `start`. */
function plainDropRun(statements: string[], start: number, tables: Tables): string[] {
  const run: string[] = [];
  for (let index = start; index < statements.length; index++) {
    const dropped = DROP_TABLE.exec(code(statements[index]!))?.[1];
    if (!dropped || isRebuild(tables, dropped)) break;
    run.push(dropped);
  }
  return run;
}

/** Orders drops so no table is dropped while another in the run points at it. */
function childrenFirst(run: string[], tables: Tables): string[] {
  const remaining = [...run];
  const ordered: string[] = [];
  while (remaining.length > 0) {
    const index = remaining.findIndex(
      (table) => !remaining.some((other) => other !== table && tables.get(other)?.has(table)),
    );
    // A cycle cannot be ordered; the rest keep their order.
    if (index === -1) return [...ordered, ...remaining];
    ordered.push(...remaining.splice(index, 1));
  }
  return ordered;
}

/** Every table that points at `table`, directly or not, nearest first. */
function dependentsOf(tables: Tables, table: string): string[] {
  const found: string[] = [];
  const queue = [table];
  while (queue.length > 0) {
    const parent = queue.shift()!;
    for (const [name, refs] of tables) {
      const skip = name === table || name === `new_${table}` || name.startsWith(BACKUP_PREFIX);
      if (!skip && refs.has(parent) && !found.includes(name)) {
        found.push(name);
        queue.push(name);
      }
    }
  }
  return found;
}

function setAside(dependents: string[]): string[] {
  const copies = dependents.map(
    (name) => `CREATE TABLE "${BACKUP_PREFIX}${name}" AS SELECT * FROM "${name}";`,
  );
  const deletes = [...dependents].reverse().map((name) => `DELETE FROM "${name}";`);
  return withComment("-- Set aside the rows that point at the rebuilt table", [
    ...copies,
    ...deletes,
  ]);
}

function putBack(dependents: string[]): string[] {
  const restores = dependents.flatMap((name) => [
    `INSERT INTO "${name}" SELECT * FROM "${BACKUP_PREFIX}${name}";`,
    `DROP TABLE "${BACKUP_PREFIX}${name}";`,
  ]);
  return withComment("-- Put the set-aside rows back", restores);
}

function withComment(comment: string, statements: string[]): string[] {
  const [first, ...rest] = statements;
  return [`${comment}\n${first}`, ...rest];
}
