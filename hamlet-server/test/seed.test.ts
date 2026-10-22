import request from "supertest";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { sqlStatements } from "../scripts/migrationFiles.ts";
import {
  MOCK_LISTINGS_QUERY,
  type MockListing,
  mockListings,
  mockReviewers,
  mockSellers,
  priceCents,
  readMockData,
  removableKeysQuery,
  removeSql,
  sessionToken,
  slug,
  standingSql,
  usersSql,
  versionOrder,
} from "../scripts/mockData.ts";
import { DEFAULT_CONFIG } from "../src/config.ts";
import { createListingSchema } from "../src/listingInput.ts";
import { readUploadedPackage } from "../src/package/index.ts";
import {
  type Harness,
  applyMigrations,
  auth,
  createHarness,
  startMiniflareD1,
} from "./support/harness.ts";

const listings = mockListings(readMockData());
const sellers = mockSellers(listings);
const reviewers = mockReviewers(listings);
const NOW = new Date("2026-10-17T00:00:00Z");

type D1 = Harness["db"];
async function exec(db: D1, sql: string): Promise<void> {
  await db.batch(sqlStatements(sql).map((statement) => db.prepare(statement)));
}

function titled(title: string): MockListing {
  return listings.find((listing) => listing.input.title === title)!;
}

describe("the mock data", () => {
  it("has the old shelves' listings, each valid as the server takes it", () => {
    const kinds = listings.map((listing) => listing.input.kind);
    expect(kinds.filter((kind) => kind === "agent")).toHaveLength(12);
    expect(kinds.filter((kind) => kind === "plugin")).toHaveLength(13);
    expect(kinds.filter((kind) => kind === "hamlet")).toHaveLength(6);
    for (const listing of listings) {
      expect(createListingSchema.parse(listing.input)).toEqual(listing.input);
      for (const version of listing.versions) {
        const pkg = readUploadedPackage(
          undefined,
          version.package,
          listing.input.kind,
          DEFAULT_CONFIG.maxPackageBytes,
        );
        expect(pkg.meta).toBeTruthy();
      }
    }
  });

  it("derives the meta the old listings showed", () => {
    const meta = (title: string) =>
      readUploadedPackage(
        undefined,
        titled(title).versions.at(-1)!.package,
        titled(title).input.kind,
        DEFAULT_CONFIG.maxPackageBytes,
      ).meta;
    expect(meta("PR Reviewer")).toMatchObject({
      name: "pr-reviewer",
      model: "anthropic/claude-sonnet-4.5",
      skills: ["code-review", "git", "test-gaps"],
      notes: true,
    });
    expect(meta("Indie SaaS Launch Team")).toMatchObject({
      name: "saas-launch",
      seed: "11",
      mayor: { name: "launch-lead", skills: ["launch-checklist"], notes: true },
    });
    expect(meta("GitHub")).toEqual({
      tools: [
        {
          name: "github-issues",
          description: expect.any(String),
          permissions: ["run_program", "network", "read_files"],
        },
        {
          name: "github-pulls",
          description: expect.any(String),
          permissions: ["run_program", "network", "read_files"],
        },
      ],
    });
    expect(meta("Mermaid Diagrams")).toEqual({
      tools: [
        { name: "mermaid-diagrams", description: expect.any(String), permissions: ["run_program"] },
      ],
    });
    expect(titled("Excalidraw Whiteboard").input.description).toContain('- Panel "Whiteboard"');
  });

  it("maps old categories, prices and sellers", () => {
    expect(titled("Meeting Notes").input).toMatchObject({ category: "Business", priceCents: 200 });
    expect(titled("Voice Mode").input.category).toBe("Language");
    const growth = sellers.find((seller) => seller.login === "growth-desk")!;
    expect(growth).toMatchObject({ githubId: "mock-growth-desk", payoutsEnabled: true });
    expect(sellers.find((seller) => seller.login === "hannah-brooks")).toMatchObject({
      payoutsEnabled: false,
      stripeAccountId: null,
    });
    expect(sellers.filter((seller) => seller.login === "rowan-patel")).toHaveLength(1);
  });
});

describe("the helpers", () => {
  it("slugs names to the package name rule", () => {
    expect(slug("PR Reviewer")).toBe("pr-reviewer");
    expect(slug("whiteboard.draw")).toBe("whiteboard-draw");
    expect(slug("Dr. Amara Osei")).toBe("dr-amara-osei");
    expect(slug("a".repeat(40))).toHaveLength(32);
    expect(() => slug("!!!")).toThrow(/valid name/);
  });

  it("maps prices to cents, monthly as once", () => {
    expect(priceCents("free")).toBe(0);
    expect(priceCents({ once: 12 })).toBe(1200);
    expect(priceCents({ monthly: 2.5 })).toBe(250);
    expect(() => priceCents({ once: 0.1 })).toThrow(/cents/);
  });

  it("orders versions oldest first, keeping only increasing ones", () => {
    const change = (version: string) => ({ version, words: version });
    expect(versionOrder([change("2.4.0"), change("2.3.0")], "2.4.0")).toEqual([
      { version: "2.3.0", changes: "2.3.0" },
      { version: "2.4.0", changes: "2.4.0" },
    ]);
    expect(
      versionOrder([change("2.0.0"), change("bad"), change("2.1.0"), change("1.0.0")], "2.0.0"),
    ).toEqual([
      { version: "1.0.0", changes: "1.0.0" },
      { version: "2.0.0", changes: "2.0.0" },
    ]);
    expect(versionOrder([], "1.2.0")).toEqual([{ version: "1.2.0", changes: "" }]);
  });
});

describe("the SQL", () => {
  let d1: Awaited<ReturnType<typeof startMiniflareD1>>;
  const t = "'2026-01-01T00:00:00.000+00:00'";
  const real = `INSERT INTO "User" ("id", "githubId", "login", "name", "avatarUrl", "payoutsEnabled", "createdAt", "updatedAt") VALUES ('u_real', '42', 'real', 'Real', '', 1, ${t}, ${t});`;
  const listing = (id: string, seller: string, status = "listed") =>
    `INSERT INTO "Listing" ("id", "kind", "title", "summary", "description", "category", "tags", "priceCents", "status", "sellerId", "createdAt", "updatedAt") VALUES ('${id}', 'agent', '${id}', 's', '', 'Code', '[]', 0, '${status}', '${seller}', ${t}, ${t});\n` +
    `INSERT INTO "Version" ("id", "listingId", "version", "major", "minor", "patch", "changes", "blobKey", "size", "sha256", "meta", "createdAt") VALUES ('v_${id}', '${id}', '1.0.0', 1, 0, 0, '', 'packages/${id}/1.0.0.json', 1, 'x', '{}', ${t});`;
  const count = async (sql: string) =>
    (await d1.db.prepare(`SELECT count(*) AS n FROM ${sql}`).first<{ n: number }>())!.n;

  beforeEach(async () => {
    d1 = await startMiniflareD1();
    await applyMigrations(d1.db, "up");
    await exec(d1.db, real);
    await exec(d1.db, usersSql(sellers, reviewers, NOW));
    const seller = sellers[0]!.id;
    await exec(
      d1.db,
      [
        listing("l_real", "u_real"),
        listing("l_mock", seller),
        listing("l_bought", seller),
        listing("l_draft", seller, "draft"),
      ].join("\n"),
    );
    await exec(
      d1.db,
      `INSERT INTO "Purchase" ("id", "listingId", "buyerId", "status", "amountCents", "feeCents", "createdAt", "updatedAt") VALUES ('p_real', 'l_bought', 'u_real', 'paid', 0, 0, ${t}, ${t});`,
    );
  });
  afterEach(async () => {
    await d1.dispose();
  });

  it("inserts users and sessions once, and refuses a real user", async () => {
    await exec(d1.db, usersSql(sellers, reviewers, NOW));
    expect(await count(`"User" WHERE "githubId" LIKE 'mock-%'`)).toBe(
      sellers.length + reviewers.length,
    );
    expect(await count(`"Session"`)).toBe(sellers.length);
    expect(() => usersSql([{ ...sellers[0]!, githubId: "42" }], [], NOW)).toThrow(/not mock/);
  });

  it("gives standing to mock listings only", async () => {
    const plan = titled("PR Reviewer");
    await exec(d1.db, standingSql("l_real", plan, 0, reviewers, NOW));
    expect(await count(`"Review"`)).toBe(0);
    expect(await count(`"Listing" WHERE "ownersCount" > 0`)).toBe(0);
    await exec(d1.db, standingSql("l_mock", plan, 0, reviewers, NOW));
    await exec(d1.db, standingSql("l_mock", plan, 0, reviewers, NOW));
    expect(await count(`"Review" WHERE "listingId" = 'l_mock'`)).toBe(3);
    expect(await count(`"Purchase" WHERE "listingId" = 'l_mock' AND "status" = 'paid'`)).toBe(3);
    const row = await d1.db
      .prepare(
        `SELECT "ownersCount", "reviewsCount", "starsAvg", "updatedAt" FROM "Listing" WHERE "id" = 'l_mock'`,
      )
      .first();
    expect(row).toEqual({
      ownersCount: 9300,
      reviewsCount: 412,
      starsAvg: 4.8,
      updatedAt: "2026-10-02T12:00:00.000+00:00",
    });
  });

  it("removes drafts, then all mock rows, but never a real user's or what one bought", async () => {
    const keys = async (removal: "all" | "drafts") =>
      (await d1.db.prepare(removableKeysQuery(removal)).all<{ key: string }>()).results.map(
        (row) => row.key,
      );
    expect(await keys("drafts")).toEqual(["packages/l_draft/1.0.0.json"]);
    await exec(d1.db, removeSql("drafts"));
    expect(await count(`"Listing"`)).toBe(3);

    await exec(d1.db, standingSql("l_mock", titled("PR Reviewer"), 0, reviewers, NOW));
    expect(await keys("all")).toEqual(["packages/l_mock/1.0.0.json"]);
    await exec(d1.db, removeSql("all"));
    const ids = (await d1.db.prepare(MOCK_LISTINGS_QUERY).all<{ id: string }>()).results;
    expect(ids.map((row) => row.id)).toEqual(["l_bought"]);
    expect(await count(`"Listing" WHERE "id" = 'l_real'`)).toBe(1);
    expect(await count(`"Purchase" WHERE "id" = 'p_real'`)).toBe(1);
    expect(await count(`"User" WHERE "githubId" LIKE 'mock-%'`)).toBe(1);
    expect(await count(`"User" WHERE "id" = 'u_real'`)).toBe(1);
    expect(await count(`"Session"`)).toBe(0);
    expect(await count(`"Review"`)).toBe(0);
  });
});

describe("seeding through the API", () => {
  let h: Harness;
  beforeAll(async () => {
    h = await createHarness();
    await exec(h.db, usersSql(sellers, reviewers, NOW));
  });
  afterAll(async () => {
    await h.dispose();
  });

  it("publishes a listing of each kind as its mock seller, with its old standing", async () => {
    for (const title of ["Study Buddy", "Excalidraw Whiteboard", "Indie SaaS Launch Team"]) {
      const plan = titled(title);
      const seller = sellers.find((candidate) => candidate.name === plan.seller.name)!;
      const as = auth(sessionToken(seller));
      const created = await request(h.app)
        .post("/v1/listings")
        .set(as)
        .send(plan.input)
        .expect(201);
      for (const version of plan.versions) {
        await request(h.app)
          .post(`/v1/listings/${created.body.id}/versions`)
          .set(as)
          .send(version)
          .expect(201);
      }
      await request(h.app).post(`/v1/listings/${created.body.id}/publish`).set(as).expect(200);
      await exec(h.db, standingSql(created.body.id, plan, 0, reviewers, NOW));
    }
    const browse = await request(h.app).get("/v1/listings?order=popular").expect(200);
    expect(
      browse.body.items.map((item: { title: string; owners: number }) => [item.title, item.owners]),
    ).toEqual([
      ["Excalidraw Whiteboard", 48200],
      ["Study Buddy", 11400],
      ["Indie SaaS Launch Team", 1260],
    ]);
    const excalidraw = browse.body.items[0];
    const detail = await request(h.app).get(`/v1/listings/${excalidraw.id}`).expect(200);
    expect(detail.body.versions.map((version: { version: string }) => version.version)).toEqual([
      "2.4.0",
      "2.3.0",
    ]);
    const buddy = browse.body.items[1];
    const reviews = await request(h.app).get(`/v1/listings/${buddy.id}`).expect(200);
    expect(reviews.body.reviews).toHaveLength(3);
    expect(buddy.seller).toMatchObject({ login: "learnwell", verified: true });
  });
});
