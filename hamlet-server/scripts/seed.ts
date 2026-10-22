/**
 * Puts the mock bazaar Hamlet used to ship (seed/mock.json) on a running
 * server as real listings of mock sellers, for local development.
 *
 *   pnpm db:seed [--reset] [--url http://localhost:8787]
 *   pnpm db:seed --remote --yes --url <server>   (not meant for production)
 *
 * Mock users, their sessions and the old standing are written with
 * Wrangler; listings, versions and publishing go through the HTTP API so
 * the server checks the packages and derives everything as for anyone.
 * Running it again adds only what is missing. `--reset` first removes
 * every mock user and all that is theirs.
 */
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { bucketName } from "./assetCatalog.ts";
import { ROOT } from "./migrationFiles.ts";
import {
  END_SESSIONS_SQL,
  KEPT_LISTINGS_QUERY,
  MOCK_LISTINGS_QUERY,
  type MockListing,
  type MockUser,
  type Removal,
  mockListings,
  mockReviewers,
  mockSellers,
  readMockData,
  removableKeysQuery,
  removeSql,
  sessionToken,
  standingSql,
  usersSql,
} from "./mockData.ts";

const DATABASE = "DB";
const LOCAL_URL = "http://localhost:8787";
// Only these may be removed; official assets live under assets/.
const LISTING_KEY_PREFIXES = ["packages/", "covers/"];

interface Options {
  reset: boolean;
  target: "--local" | "--remote";
  url: string;
}

async function main(): Promise<void> {
  const options = parseArgs(process.argv.slice(2));
  await checkHealth(options.url);

  const listings = mockListings(readMockData());
  const sellers = mockSellers(listings);
  const reviewers = mockReviewers(listings);

  if (options.reset) {
    removeListings(options, "all");
  } else {
    removeListings(options, "drafts");
  }

  const existing = new Set(
    query(options, MOCK_LISTINGS_QUERY).map((row) =>
      listingKey(String(row.kind), String(row.githubId), String(row.title)),
    ),
  );
  const sellerOf = (listing: MockListing) =>
    sellers.find((seller) => seller.name === listing.seller.name)!;
  const missing = listings.filter(
    (listing) =>
      !existing.has(
        listingKey(listing.input.kind, sellerOf(listing).githubId, listing.input.title),
      ),
  );
  if (missing.length === 0) {
    console.log(`mock data is already there (${listings.length} listings); --reset starts over`);
    return;
  }

  execute(options, usersSql(sellers, reviewers, new Date()));
  const standing: string[] = [];
  for (const listing of missing) {
    const id = await createListing(options.url, sellerOf(listing), listing);
    standing.push(standingSql(id, listing, listings.indexOf(listing), reviewers, new Date()));
    console.log(`${listing.input.kind} ${listing.input.title}: ${id}`);
  }
  execute(options, standing.join("\n"));
  if (options.target === "--remote") execute(options, END_SESSIONS_SQL);

  const counts = ["agent", "plugin", "hamlet"].map(
    (kind) => `${missing.filter((listing) => listing.input.kind === kind).length} ${kind}s`,
  );
  console.log(`seeded ${counts.join(", ")} by ${sellers.length} mock sellers`);
}

function parseArgs(args: string[]): Options {
  const known = new Set(["--reset", "--local", "--remote", "--yes", "--url"]);
  const urlIndex = args.indexOf("--url");
  const url = urlIndex === -1 ? undefined : args[urlIndex + 1];
  if (urlIndex !== -1 && (!url || url.startsWith("--"))) fail("--url needs the server's URL");
  const unknown = args.filter(
    (arg, index) => !known.has(arg) && (urlIndex === -1 || index !== urlIndex + 1),
  );
  if (unknown.length > 0) fail(`unknown argument ${unknown[0]}; see scripts/seed.ts`);
  const remote = args.includes("--remote");
  if (remote && args.includes("--local")) fail("give --local or --remote, not both");
  if (remote && !args.includes("--yes")) {
    fail(
      "mock data does not belong in production. To put it in the remote database anyway, " +
        "add --yes and the server's --url.",
    );
  }
  if (remote && !url) fail("with --remote, give the server's --url");
  return {
    reset: args.includes("--reset"),
    target: remote ? "--remote" : "--local",
    url: (url ?? LOCAL_URL).replace(/\/+$/, ""),
  };
}

async function checkHealth(url: string): Promise<void> {
  const ok = await fetch(`${url}/health`).then(
    (response) => response.ok,
    () => false,
  );
  if (!ok) fail(`no hamlet-server answers at ${url}/health; start it with pnpm dev first`);
}

function listingKey(kind: string, githubId: string, title: string): string {
  return JSON.stringify([kind, githubId, title]);
}

/** Removes mock listings and their R2 objects, keeping any a real user bought or reviewed. */
function removeListings(options: Options, removal: Removal): void {
  const keys = query(options, removableKeysQuery(removal)).map((row) => String(row.key));
  const unsafe = keys.find((key) => !LISTING_KEY_PREFIXES.some((prefix) => key.startsWith(prefix)));
  if (unsafe) fail(`refusing to delete R2 object ${unsafe}`);
  if (removal === "all") {
    for (const row of query(options, KEPT_LISTINGS_QUERY)) {
      console.log(`keeping ${String(row.title)}: a real user bought or reviewed it`);
    }
  }
  execute(options, removeSql(removal));
  const bucket = bucketName(readFileSync(join(ROOT, "wrangler.jsonc"), "utf8"));
  for (const key of keys) {
    capture(["r2", "object", "delete", `${bucket}/${key}`, options.target]);
  }
  if (removal === "all") console.log(`removed mock data and ${keys.length} R2 objects`);
}

async function createListing(url: string, seller: MockUser, listing: MockListing) {
  const call = (path: string, body: unknown) => api(url, sessionToken(seller), path, body);
  const created = (await call("/v1/listings", listing.input)) as { id: string };
  for (const version of listing.versions) {
    await call(`/v1/listings/${created.id}/versions`, version);
  }
  await call(`/v1/listings/${created.id}/publish`, {});
  return created.id;
}

async function api(url: string, token: string, path: string, body: unknown): Promise<unknown> {
  const response = await fetch(`${url}${path}`, {
    method: "POST",
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const text = await response.text();
  if (!response.ok) fail(`POST ${path}: ${response.status} ${text}`);
  return JSON.parse(text) as unknown;
}

function query(options: Options, sql: string): Record<string, unknown>[] {
  const output = capture(["d1", "execute", DATABASE, options.target, "--json", "--command", sql]);
  const parsed = JSON.parse(output) as { results?: Record<string, unknown>[] }[];
  return parsed[0]?.results ?? [];
}

function execute(options: Options, sql: string): void {
  const dir = join(ROOT, ".wrangler");
  mkdirSync(dir, { recursive: true });
  const file = join(dir, "db-seed.sql");
  writeFileSync(file, sql);
  capture(["d1", "execute", DATABASE, options.target, "--yes", "--file", file]);
}

function capture(args: string[]): string {
  const result = spawnSync(join(ROOT, "node_modules", ".bin", "wrangler"), args, {
    cwd: ROOT,
    encoding: "utf8",
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

await main();
