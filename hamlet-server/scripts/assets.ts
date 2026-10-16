/**
 * Publishes the official assets the baker wrote (see docs/assets.md).
 *
 *   pnpm assets:publish [--local|--remote] [--dir assets-src/out]
 *
 * Uploads every file and preview to R2, then upserts the catalog into D1
 * as listed and unlists any asset missing from it. Local by default.
 */
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { bucketName, missingFiles, parseCatalog, publishSql, uploadsFor } from "./assetCatalog.ts";
import { ROOT } from "./migrationFiles.ts";

const DATABASE = "DB";
const DEFAULT_DIR = "assets-src/out";

function main(): void {
  const args = process.argv.slice(2);
  const remote = args.includes("--remote");
  if (remote && args.includes("--local")) fail("give --local or --remote, not both");
  const target = remote ? "--remote" : "--local";
  const dir = catalogDir(args);

  let entries;
  try {
    entries = parseCatalog(JSON.parse(readFileSync(join(dir, "catalog.json"), "utf8")));
  } catch (error) {
    fail(`${join(dir, "catalog.json")}: ${(error as Error).message}`);
  }
  const uploads = uploadsFor(dir, entries);
  const missing = missingFiles(uploads);
  if (missing.length > 0) fail(`missing files:\n  ${missing.join("\n  ")}`);

  const bucket = bucketName(readFileSync(join(ROOT, "wrangler.jsonc"), "utf8"));
  for (const upload of uploads) {
    console.log(`r2 ${target.slice(2)}: ${upload.key}`);
    run([
      "r2",
      "object",
      "put",
      `${bucket}/${upload.key}`,
      "--file",
      upload.path,
      "--content-type",
      upload.contentType,
      target,
    ]);
  }

  const sqlDir = join(ROOT, ".wrangler");
  mkdirSync(sqlDir, { recursive: true });
  const sqlFile = join(sqlDir, "assets-publish.sql");
  writeFileSync(sqlFile, publishSql(entries, new Date()));
  run(["d1", "execute", DATABASE, target, "--yes", "--file", sqlFile]);
  console.log(`published ${entries.length} assets, ${uploads.length} files (${target.slice(2)})`);
}

/** `--dir` is taken from where pnpm was run; the default is in hamlet-server. */
function catalogDir(args: string[]): string {
  const index = args.indexOf("--dir");
  if (index === -1) return join(ROOT, DEFAULT_DIR);
  const dir = args[index + 1];
  if (!dir || dir.startsWith("--")) fail("--dir needs a folder");
  return resolve(process.env.INIT_CWD ?? process.cwd(), dir);
}

function run(args: string[]): void {
  const result = spawnSync(join(ROOT, "node_modules", ".bin", "wrangler"), args, {
    cwd: ROOT,
    stdio: "inherit",
  });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

main();
