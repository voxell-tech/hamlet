/**
 * The pure parts of `pnpm db:seed`: reading `seed/mock.json` (the mock
 * bazaar Hamlet shipped before it had a server), turning it into listings
 * and packages, and writing the SQL that only ever touches mock rows.
 * Nothing here runs Wrangler or talks HTTP, so the tests can use all of it.
 */
import { Buffer } from "node:buffer";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { sha256Hex } from "../src/ids.ts";
import { MAX_PRICE_CENTS, MIN_PAID_CENTS, isValidPrice, platformFeeCents } from "../src/money.ts";
import type { Package, PackageFile } from "../src/package/schema.ts";
import { NAME_PATTERN } from "../src/package/paths.ts";
import { STAFF_ACCOUNT } from "../src/payouts.ts";
import { compareVersions, parseVersion } from "../src/versionNumber.ts";
import { ROOT } from "./migrationFiles.ts";

export const MOCK_DATA_PATH = join(ROOT, "seed", "mock.json");
/** Every mock user's GitHub id starts with this; no real one can. */
export const MOCK_PREFIX = "mock-";
export const REVIEWS_PER_LISTING = 3;
const FEE_PERCENT = 10;
const SUMMARY_CHARS = 140;
const SESSION_EXPIRES = "2099-01-01T00:00:00.000+00:00";
const VERSION_GAP_DAYS = 30;
const DAY_MS = 24 * 60 * 60 * 1000;

const sellerSchema = z.object({ name: z.string(), verified: z.boolean() });
const priceSchema = z.union([
  z.literal("free"),
  z.object({ once: z.number() }),
  z.object({ monthly: z.number() }),
]);
const standingSchema = z.object({
  stars: z.number().min(0).max(5),
  reviews: z.number().int().min(0),
  owners: z.number().int().min(0),
});
const daySchema = z.string().regex(/^\d{4}-\d{2}-\d{2}$/);
const reviewSchema = z.object({
  by: z.string(),
  stars: z.number().int().min(1).max(5),
  words: z.string(),
});
const skillSchema = z.object({ name: z.string(), description: z.string(), body: z.string() });
const settlerSchema = z.object({
  name: z.string(),
  purpose: z.string(),
  character: z.string(),
  notes: z.string(),
  skills: z.array(skillSchema),
});
const permissionSchema = z.union([
  z.enum([
    "ReadWorkspace",
    "WriteWorkspace",
    "RunCommands",
    "Speakers",
    "Microphone",
    "Notifications",
  ]),
  z.object({ Network: z.string() }),
]);

const CRAFTS = {
  Writing: "Writing",
  Code: "Code",
  Research: "Research",
  Trade: "Business",
  Household: "Personal",
  Teaching: "Learning",
} as const;

// Themes is no shelf of the client any more; a listing in it shows only
// under all of them.
const SHELVES = {
  Drawing: "Diagrams",
  Planning: "Productivity",
  Data: "Data",
  Language: "Language",
  Maps: "Integrations",
  Looks: "Themes",
} as const;

const PARTS = {
  Panel: "Panel",
  Tool: "Tool for agents",
  Command: "Command",
  InChat: "In the chat",
  Theme: "Theme",
} as const;

const mockDataSchema = z.object({
  agents: z.array(
    z.object({
      name: z.string(),
      seller: sellerSchema,
      purpose: z.string(),
      character: z.string(),
      skills: z.array(z.string()),
      model: z.string(),
      craft: z.enum(Object.keys(CRAFTS) as [keyof typeof CRAFTS]),
      tags: z.array(z.string()),
      price: priceSchema,
      standing: standingSchema,
      reviews: z.array(reviewSchema),
      version: z.string(),
      updated: daySchema,
    }),
  ),
  plugins: z.array(
    z.object({
      name: z.string(),
      seller: sellerSchema,
      version: z.string(),
      updated: daySchema,
      price: priceSchema,
      standing: standingSchema,
      summary: z.string(),
      description: z.string(),
      shelf: z.enum(Object.keys(SHELVES) as [keyof typeof SHELVES]),
      adds: z.array(
        z.object({
          part: z.enum(Object.keys(PARTS) as [keyof typeof PARTS]),
          name: z.string(),
          words: z.string(),
        }),
      ),
      asks: z.array(permissionSchema),
      changes: z.array(z.object({ version: z.string(), words: z.string() })),
    }),
  ),
  hamlets: z.array(
    z.object({
      name: z.string(),
      title: z.string(),
      kind: z.string(),
      description: z.string(),
      purpose: z.string(),
      seller: sellerSchema,
      price: priceSchema,
      standing: standingSchema,
      seed: z.string(),
      version: z.string(),
      updated: daySchema,
      mayor: settlerSchema,
      agents: z.array(settlerSchema),
    }),
  ),
});

export type MockData = z.infer<typeof mockDataSchema>;
export type MockAgent = MockData["agents"][number];
export type MockPlugin = MockData["plugins"][number];
export type MockHamlet = MockData["hamlets"][number];
export type Price = z.infer<typeof priceSchema>;
type Seller = z.infer<typeof sellerSchema>;
type Standing = z.infer<typeof standingSchema>;
type Review = z.infer<typeof reviewSchema>;
type Settler = z.infer<typeof settlerSchema>;
type Permission = z.infer<typeof permissionSchema>;

export function readMockData(path = MOCK_DATA_PATH): MockData {
  return mockDataSchema.parse(JSON.parse(readFileSync(path, "utf8")));
}

/** `text` as a package name: lowercase words joined by `-`, at most 32 chars. */
export function slug(text: string): string {
  const name = text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+/, "")
    .slice(0, 32)
    .replace(/-+$/, "");
  if (!NAME_PATTERN.test(name)) throw new Error(`"${text}" makes no valid name`);
  return name;
}

/** A monthly price becomes the same amount paid once; the server has no subscriptions. */
export function priceCents(price: Price): number {
  const dollars = price === "free" ? 0 : "once" in price ? price.once : price.monthly;
  const cents = Math.round(dollars * 100);
  if (!isValidPrice(cents)) {
    throw new Error(`${cents} cents is not 0 or ${MIN_PAID_CENTS} to ${MAX_PRICE_CENTS}`);
  }
  return cents;
}

/** Old `changes` (latest first) as uploads: oldest first, each greater than the last, `latest` last. */
export function versionOrder(
  changes: { version: string; words: string }[],
  latest: string,
): { version: string; changes: string }[] {
  const latestNumber = parseVersion(latest);
  if (!latestNumber) throw new Error(`version "${latest}" is not MAJOR.MINOR.PATCH`);
  const kept: { version: string; changes: string }[] = [];
  for (const change of [...changes].reverse()) {
    const number = parseVersion(change.version);
    if (!number || compareVersions(number, latestNumber) > 0) continue;
    const previous = kept.at(-1);
    if (previous && compareVersions(number, parseVersion(previous.version)!) <= 0) continue;
    kept.push({ version: change.version, changes: change.words });
  }
  if (kept.at(-1)?.version !== latest) kept.push({ version: latest, changes: "" });
  return kept;
}

/** At most 140 chars: the text itself, or its first sentence. */
export function fitSummary(text: string): string {
  if (text.length <= SUMMARY_CHARS) return text;
  const sentence = /^.*?[.!?](\s|$)/.exec(text)?.[0].trim();
  if (sentence && sentence.length <= SUMMARY_CHARS) return sentence;
  return `${text.slice(0, SUMMARY_CHARS - 3).trimEnd()}...`;
}

function file(path: string, text: string, executable?: boolean): PackageFile {
  return {
    path,
    data: Buffer.from(text, "utf8").toString("base64"),
    ...(executable ? { executable } : {}),
  };
}

function skillFile(name: string, description: string, body: string): string {
  return `---\nname: ${name}\ndescription: ${description}\n---\n\n${body.trim()}\n`;
}

export function agentPackage(agent: MockAgent): Package {
  return {
    format: 1,
    kind: "agent",
    agent: {
      name: slug(agent.name),
      purpose: agent.purpose,
      model: agent.model,
      files: [
        file("CHARACTER.md", `${agent.character}\n`),
        file("workspace/NOTES.md", "# Notes\n"),
        ...agent.skills.map((skill) => {
          const name = slug(skill);
          const description = `${agent.name}'s ${skill.replaceAll("-", " ")} skill.`;
          return file(
            `workspace/skills/${name}/SKILL.md`,
            skillFile(name, description, `Use this skill for ${skill.replaceAll("-", " ")}.`),
          );
        }),
      ],
    },
  };
}

function settlerAgent(settler: Settler) {
  return {
    name: slug(settler.name),
    purpose: settler.purpose,
    model: null,
    files: [
      file("CHARACTER.md", `${settler.character}\n`),
      file("workspace/NOTES.md", settler.notes),
      ...settler.skills.map((skill) =>
        file(
          `workspace/skills/${slug(skill.name)}/SKILL.md`,
          skillFile(slug(skill.name), skill.description, skill.body),
        ),
      ),
    ],
  };
}

export function hamletPackage(hamlet: MockHamlet): Package {
  return {
    format: 1,
    kind: "hamlet",
    hamlet: {
      name: slug(hamlet.name),
      title: hamlet.title,
      kind: hamlet.kind,
      purpose: hamlet.purpose,
      seed: hamlet.seed,
      mayor: settlerAgent(hamlet.mayor),
      agents: hamlet.agents.map(settlerAgent),
    },
  };
}

/** One tool per old `Tool` addition, or one named after the plugin when it had none. */
export function pluginPackage(plugin: MockPlugin, version: string): Package {
  const tools = plugin.adds.filter((add) => add.part === "Tool");
  const named = tools.length > 0 ? tools : [{ name: plugin.name, words: plugin.summary }];
  const sandbox = {
    ...(plugin.asks.some((ask) => typeof ask === "object") ? { net: true } : {}),
    ...(plugin.asks.includes("WriteWorkspace") ? { fs_rw: ["out"] } : {}),
    ...(plugin.asks.includes("ReadWorkspace") ? { fs_ro: ["."] } : {}),
  };
  return {
    format: 1,
    kind: "plugin",
    plugin: {
      tools: named.map((tool) => {
        const name = slug(tool.name);
        const manifest = {
          name,
          description: tool.words,
          exec: ["sh", "run.sh"],
          parameters: { type: "object", properties: {} },
          sandbox,
        };
        const script = `#!/bin/sh\necho "${name} ${version} is a mock tool from Hamlet's seed data; it does nothing."\n`;
        return {
          name,
          files: [
            file("manifest.json", `${JSON.stringify(manifest, null, 2)}\n`),
            file("run.sh", script, true),
          ],
        };
      }),
    },
  };
}

function permissionWords(permission: Permission): string {
  if (typeof permission === "object") return `Reach ${permission.Network}`;
  return {
    ReadWorkspace: "Read the files of the workspace",
    WriteWorkspace: "Write files in the workspace",
    RunCommands: "Run commands on this computer",
    Speakers: "Play sound",
    Microphone: "Listen through the microphone",
    Notifications: "Send you notifications",
  }[permission];
}

/** The old description, then what the plugin added besides tools and what it asked for. */
export function pluginDescription(plugin: MockPlugin): string {
  const others = plugin.adds.filter((add) => add.part !== "Tool");
  const parts = [plugin.description];
  if (others.length > 0) {
    const lines = others.map((add) => `- ${PARTS[add.part]} "${add.name}": ${add.words}`);
    parts.push(`Also adds:\n${lines.join("\n")}`);
  }
  if (plugin.asks.length > 0) {
    parts.push(`Asks to:\n${plugin.asks.map((ask) => `- ${permissionWords(ask)}`).join("\n")}`);
  }
  return parts.join("\n\n");
}

export interface ListingInput {
  kind: "agent" | "hamlet" | "plugin";
  title: string;
  summary: string;
  description: string;
  category: string;
  tags: string[];
  priceCents: number;
}

/** One mock listing: what to create, upload and record as its standing. */
export interface MockListing {
  input: ListingInput;
  seller: Seller;
  versions: { version: string; changes: string; package: Package }[];
  standing: Standing;
  reviews: Review[];
  /** The old `updated` day, `YYYY-MM-DD`. */
  updated: string;
}

export function mockListings(data: MockData): MockListing[] {
  const agents = data.agents.map((agent): MockListing => ({
    input: {
      kind: "agent",
      title: agent.name,
      summary: fitSummary(agent.purpose),
      description: "",
      category: CRAFTS[agent.craft],
      tags: agent.tags,
      priceCents: priceCents(agent.price),
    },
    seller: agent.seller,
    versions: [{ version: agent.version, changes: "", package: agentPackage(agent) }],
    standing: agent.standing,
    reviews: agent.reviews,
    updated: agent.updated,
  }));
  const plugins = data.plugins.map((plugin): MockListing => ({
    input: {
      kind: "plugin",
      title: plugin.name,
      summary: fitSummary(plugin.summary),
      description: pluginDescription(plugin),
      category: SHELVES[plugin.shelf],
      tags: [],
      priceCents: priceCents(plugin.price),
    },
    seller: plugin.seller,
    versions: versionOrder(plugin.changes, plugin.version).map((step) => ({
      ...step,
      package: pluginPackage(plugin, step.version),
    })),
    standing: plugin.standing,
    reviews: [],
    updated: plugin.updated,
  }));
  const hamlets = data.hamlets.map((hamlet): MockListing => ({
    input: {
      kind: "hamlet",
      title: hamlet.title,
      summary: fitSummary(hamlet.description),
      description: hamlet.description,
      category: hamlet.kind,
      tags: [],
      priceCents: priceCents(hamlet.price),
    },
    seller: hamlet.seller,
    versions: [{ version: hamlet.version, changes: "", package: hamletPackage(hamlet) }],
    standing: hamlet.standing,
    reviews: [],
    updated: hamlet.updated,
  }));
  return [...agents, ...plugins, ...hamlets];
}

/** A mock user: a seller, or a reviewer standing for the old owners. */
export interface MockUser {
  id: string;
  githubId: string;
  login: string;
  name: string;
  payoutsEnabled: boolean;
  stripeAccountId: string | null;
}

/**
 * The sellers of `listings`. Payouts are on for the verified and for any
 * with a paid listing, which cannot be published without them; the store
 * shows them all as verified.
 */
export function mockSellers(listings: MockListing[]): MockUser[] {
  const sellers = new Map<string, MockUser>();
  for (const listing of listings) {
    const login = slug(listing.seller.name);
    const enabled = listing.seller.verified || listing.input.priceCents > 0;
    const known = sellers.get(login);
    if (known) {
      if (enabled && !known.payoutsEnabled) Object.assign(known, payouts(login));
      continue;
    }
    sellers.set(login, {
      id: `u_mock_${login}`,
      githubId: `${MOCK_PREFIX}${login}`,
      login,
      name: listing.seller.name,
      ...(enabled ? payouts(login) : { payoutsEnabled: false, stripeAccountId: null }),
    });
  }
  return [...sellers.values()];
}

function payouts(login: string) {
  return { payoutsEnabled: true, stripeAccountId: `${STAFF_ACCOUNT}${login}` };
}

/** The session token the seed script uses for `seller`; only its hash is stored. */
export function sessionToken(seller: MockUser): string {
  return `hms_mock_${seller.login}`;
}

// Stand-ins for the many owners the old reviews do not name.
const OTHER_REVIEWERS = ["Alex H.", "Chen W.", "Dana O.", "Eli V.", "Farah Q.", "Gus N."];
const OTHER_WORDS = [
  "Does what it says.",
  "Worth it.",
  "Saves me time every week.",
  "Solid, a few rough edges.",
  "Set it up in minutes.",
  "Would get it again.",
];

/** Every reviewer `mockReviews` uses, in a fixed order. */
export function mockReviewers(listings: MockListing[]): MockUser[] {
  const names = [...new Set(listings.flatMap((listing) => listing.reviews.map((r) => r.by)))];
  return [...names, ...OTHER_REVIEWERS].map((name, index) => ({
    id: `u_mock_reviewer_${index + 1}`,
    githubId: `${MOCK_PREFIX}reviewer-${index + 1}`,
    login: slug(name),
    name,
    payoutsEnabled: false,
    stripeAccountId: null,
  }));
}

/** The old reviews of `listing`, then made-up ones up to `REVIEWS_PER_LISTING`. */
export function mockReviews(listing: MockListing, index: number): Review[] {
  const reviews = listing.reviews.slice(0, REVIEWS_PER_LISTING);
  const wanted = Math.min(REVIEWS_PER_LISTING, listing.standing.reviews);
  for (let extra = 0; reviews.length < wanted; extra++) {
    const pick = (index + extra) % OTHER_REVIEWERS.length;
    const by = OTHER_REVIEWERS[pick]!;
    if (reviews.some((review) => review.by === by)) continue;
    reviews.push({
      by,
      stars: Math.min(5, Math.max(1, Math.round(listing.standing.stars - (extra % 2) * 0.5))),
      words: OTHER_WORDS[(index + extra) % OTHER_WORDS.length]!,
    });
  }
  return reviews;
}

/** How Prisma's D1 adapter stores a DateTime, so Prisma reads it back. */
export function sqliteTime(date: Date): string {
  return date.toISOString().replace("Z", "+00:00");
}

function dayTime(day: string, plusDays = 0): Date {
  return new Date(Date.parse(`${day}T12:00:00Z`) + plusDays * DAY_MS);
}

function sql(value: string | number | boolean | null): string {
  if (value === null) return "NULL";
  if (typeof value === "boolean") return value ? "1" : "0";
  if (typeof value === "number") return String(value);
  // One line each, so every SQL statement stays on one line.
  if (/\p{Cc}/u.test(value)) throw new Error(`no control chars in SQL text: ${value}`);
  return `'${value.replaceAll("'", "''")}'`;
}

const MOCK_USERS = `SELECT "id" FROM "User" WHERE substr("githubId", 1, ${MOCK_PREFIX.length}) = ${sql(MOCK_PREFIX)}`;
const MOCK_LISTINGS = `SELECT "id" FROM "Listing" WHERE "sellerId" IN (${MOCK_USERS})`;
/** Listings anyone but a mock user has bought or reviewed; never deleted. */
const TOUCHED_LISTINGS =
  `SELECT "listingId" FROM "Purchase" WHERE "buyerId" NOT IN (${MOCK_USERS}) ` +
  `UNION SELECT "listingId" FROM "Review" WHERE "userId" NOT IN (${MOCK_USERS})`;

function userId(githubId: string): string {
  if (!githubId.startsWith(MOCK_PREFIX)) throw new Error(`${githubId} is not a mock user`);
  return `(SELECT "id" FROM "User" WHERE "githubId" = ${sql(githubId)})`;
}

/** Inserts the mock users, or brings existing ones up to date, and the sellers' sessions. */
export function usersSql(sellers: MockUser[], reviewers: MockUser[], now: Date): string {
  const time = sql(sqliteTime(now));
  const users = [...sellers, ...reviewers].map((user) => {
    if (!user.githubId.startsWith(MOCK_PREFIX)) throw new Error(`${user.githubId} is not mock`);
    return (
      `INSERT INTO "User" ("id", "githubId", "login", "name", "avatarUrl", "stripeAccountId", "payoutsEnabled", "createdAt", "updatedAt") ` +
      `VALUES (${sql(user.id)}, ${sql(user.githubId)}, ${sql(user.login)}, ${sql(user.name)}, '', ${sql(user.stripeAccountId)}, ${sql(user.payoutsEnabled)}, ${time}, ${time}) ` +
      `ON CONFLICT ("githubId") DO UPDATE SET "login" = excluded."login", "name" = excluded."name", ` +
      `"stripeAccountId" = excluded."stripeAccountId", "payoutsEnabled" = excluded."payoutsEnabled", "updatedAt" = excluded."updatedAt" ` +
      `WHERE substr("User"."githubId", 1, ${MOCK_PREFIX.length}) = ${sql(MOCK_PREFIX)};`
    );
  });
  const sessions = sellers.map(
    (seller) =>
      `INSERT INTO "Session" ("id", "tokenHash", "userId", "expiresAt", "createdAt") ` +
      `VALUES (${sql(`s_mock_${seller.login}`)}, ${sql(sha256Hex(sessionToken(seller)))}, ${userId(seller.githubId)}, ${sql(SESSION_EXPIRES)}, ${time}) ` +
      `ON CONFLICT ("id") DO UPDATE SET "expiresAt" = excluded."expiresAt";`,
  );
  return ["-- Written by pnpm db:seed.", ...users, ...sessions, ""].join("\n");
}

/**
 * Gives a just published mock listing its old standing: a few paid
 * purchases and reviews by mock reviewers, the old counters (more than
 * those rows) and dates from its old `updated` day.
 */
export function standingSql(
  listingId: string,
  listing: MockListing,
  index: number,
  reviewers: MockUser[],
  now: Date,
): string {
  const id = sql(listingId);
  const mine = `${id} IN (${MOCK_LISTINGS})`;
  const statements: string[] = [];
  const price = listing.input.priceCents;
  const latest = dayTime(listing.updated);
  mockReviews(listing, index).forEach((review, n) => {
    const reviewer = reviewers.find((candidate) => candidate.name === review.by);
    if (!reviewer) throw new Error(`no mock reviewer named ${review.by}`);
    const at = sql(
      sqliteTime(new Date(Math.min(dayTime(listing.updated, n + 1).getTime(), now.getTime()))),
    );
    statements.push(
      `INSERT INTO "Purchase" ("id", "listingId", "buyerId", "status", "amountCents", "feeCents", "stripeSessionId", "paidAt", "createdAt", "updatedAt") ` +
        `SELECT ${sql(`p_mock_${listingId}_${n}`)}, ${id}, ${userId(reviewer.githubId)}, 'paid', ${price}, ${platformFeeCents(price, FEE_PERCENT)}, NULL, ${at}, ${at}, ${at} ` +
        `WHERE ${mine} ON CONFLICT DO NOTHING;`,
      `INSERT INTO "Review" ("id", "listingId", "userId", "stars", "words", "createdAt", "updatedAt") ` +
        `SELECT ${sql(`r_mock_${listingId}_${n}`)}, ${id}, ${userId(reviewer.githubId)}, ${review.stars}, ${sql(review.words)}, ${at}, ${at} ` +
        `WHERE ${mine} ON CONFLICT DO NOTHING;`,
    );
  });
  const count = listing.versions.length;
  listing.versions.forEach((version, n) => {
    const at = sqliteTime(dayTime(listing.updated, -(count - 1 - n) * VERSION_GAP_DAYS));
    statements.push(
      `UPDATE "Version" SET "createdAt" = ${sql(at)} WHERE "listingId" = ${id} AND "version" = ${sql(version.version)} AND ${mine};`,
    );
  });
  const created = sqliteTime(dayTime(listing.updated, -(count - 1) * VERSION_GAP_DAYS));
  const { owners, reviews, stars } = listing.standing;
  statements.push(
    `UPDATE "Listing" SET "ownersCount" = ${owners}, "reviewsCount" = ${reviews}, "starsAvg" = ${stars}, ` +
      `"createdAt" = ${sql(created)}, "updatedAt" = ${sql(sqliteTime(latest))} WHERE "id" = ${id} AND ${mine};`,
  );
  return statements.join("\n");
}

/** Mock listings, with their seller's GitHub id. */
export const MOCK_LISTINGS_QUERY =
  `SELECT "Listing"."id" AS "id", "kind", "title", "status", "githubId" FROM "Listing" ` +
  `JOIN "User" ON "User"."id" = "Listing"."sellerId" WHERE "Listing"."id" IN (${MOCK_LISTINGS})`;

/** Which mock listings to remove: all of them, or only drafts left by a failed run. */
export type Removal = "all" | "drafts";

function removable(removal: Removal): string {
  const drafts = removal === "drafts" ? ` AND "status" = 'draft'` : "";
  return `"id" IN (${MOCK_LISTINGS}) AND "id" NOT IN (${TOUCHED_LISTINGS})${drafts}`;
}

/** The R2 keys (packages and covers) of the listings `removeSql` removes. */
export function removableKeysQuery(removal: Removal): string {
  return (
    `SELECT "blobKey" AS "key" FROM "Version" WHERE "listingId" IN (SELECT "id" FROM "Listing" WHERE ${removable(removal)}) ` +
    `UNION SELECT "coverKey" AS "key" FROM "Listing" WHERE "coverKey" IS NOT NULL AND ${removable(removal)}`
  );
}

/** Mock listings a real user bought or reviewed; `removeSql` keeps them. */
export const KEPT_LISTINGS_QUERY = `SELECT "id", "title" FROM "Listing" WHERE "id" IN (${MOCK_LISTINGS}) AND "id" IN (${TOUCHED_LISTINGS})`;

/** Ends the mock sellers' sessions, whose tokens anyone reading this script knows. */
export const END_SESSIONS_SQL = `DELETE FROM "Session" WHERE "userId" IN (${MOCK_USERS});`;

/**
 * Removes mock listings (their versions, purchases and reviews go with
 * them). With `all`, also every purchase, review and session of a mock
 * user, and the mock users left with no listing. Rows of real users are
 * never touched, and nor is a listing one of them bought or reviewed.
 */
export function removeSql(removal: Removal): string {
  const statements = [`DELETE FROM "Listing" WHERE ${removable(removal)};`];
  if (removal === "all") {
    statements.push(
      `DELETE FROM "Review" WHERE "userId" IN (${MOCK_USERS});`,
      `DELETE FROM "Purchase" WHERE "buyerId" IN (${MOCK_USERS});`,
      END_SESSIONS_SQL,
      `DELETE FROM "User" WHERE "id" IN (${MOCK_USERS}) AND "id" NOT IN (SELECT "sellerId" FROM "Listing");`,
    );
  }
  return ["-- Written by pnpm db:seed.", ...statements, ""].join("\n");
}
