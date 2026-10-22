import request from "supertest";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createListing, listedListing, uploadVersion } from "./support/flows.ts";
import { type Harness, auth, createHarness } from "./support/harness.ts";

let h: Harness;
let alice: string;
let bob: string;
let carol: string;
let admin: string;
const ids: Record<string, string> = {};

beforeAll(async () => {
  h = await createHarness();
  alice = await h.signInSeller("alice");
  bob = await h.signInSeller("bob-builder");
  carol = await h.signIn("carol");
  admin = await h.signIn("admin");

  ids.reviewer = (
    await listedListing(h, alice, {
      title: "PR Reviewer",
      summary: "Reviews pull requests.",
      category: "Code",
      tags: ["github", "review"],
      priceCents: 900,
    })
  ).id;
  ids.crew = (
    await listedListing(h, bob, {
      kind: "hamlet",
      title: "Launch Crew",
      summary: "Ships a small product.",
      category: "Business",
      priceCents: 0,
    })
  ).id;
  ids.calendar = (
    await listedListing(h, alice, {
      kind: "plugin",
      title: "Calendar",
      summary: "Reads your calendar.",
      category: "Tools",
      tags: ["time"],
      priceCents: 50,
    })
  ).id;
  ids.writer = (
    await listedListing(h, bob, {
      title: "Doc Writer",
      summary: "Writes docs for GitHub projects.",
      category: "Writing",
      priceCents: 2500,
    })
  ).id;
  ids.draft = (await createListing(h, alice, { title: "Draft Agent" })).id;
  ids.noVersion = (await createListing(h, alice, { title: "Empty" })).id;
  const unlisted = await listedListing(h, alice, { title: "Unlisted Agent" });
  await request(h.app).post(`/v1/listings/${unlisted.id}/unlist`).set(auth(alice)).expect(200);
  ids.unlisted = unlisted.id;
  const removed = await listedListing(h, alice, { title: "Removed Agent" });
  await request(h.app)
    .post(`/v1/admin/listings/${removed.id}/takedown`)
    .set(auth(admin))
    .send({ reason: "spam" })
    .expect(200);
  ids.removed = removed.id;

  // Popularity: the crew has two owners, the reviewer one (set directly
  // here; buying is covered in buying.test.ts).
  await h.prisma.listing.update({ where: { id: ids.crew! }, data: { ownersCount: 2 } });
  await h.prisma.listing.update({ where: { id: ids.reviewer! }, data: { ownersCount: 1 } });
  // Ratings: the calendar has the best mean, the writer and reviewer tie on it.
  await h.prisma.listing.update({
    where: { id: ids.calendar! },
    data: { starsAvg: 5, reviewsCount: 1 },
  });
  await h.prisma.listing.update({
    where: { id: ids.writer! },
    data: { starsAvg: 4, reviewsCount: 3 },
  });
  await h.prisma.listing.update({
    where: { id: ids.reviewer! },
    data: { starsAvg: 4, reviewsCount: 2 },
  });
  // The reviewer gets a new version, so it is the newest.
  await uploadVersion(h, alice, ids.reviewer!, "1.1.0").expect(201);
});
afterAll(async () => {
  await h.dispose();
});

async function browse(query: Record<string, string | number> = {}) {
  const response = await request(h.app).get("/v1/listings").query(query).expect(200);
  return {
    titles: (response.body.items as { title: string }[]).map((item) => item.title),
    total: response.body.total as number,
    items: response.body.items as Record<string, unknown>[],
  };
}

describe("browsing", () => {
  it("shows only listed listings with a version, most popular first", async () => {
    const { titles, total } = await browse();
    expect(titles).toEqual(["Launch Crew", "PR Reviewer", "Calendar", "Doc Writer"]);
    expect(total).toBe(4);
  });

  it("filters by kind", async () => {
    expect((await browse({ kind: "plugin" })).titles).toEqual(["Calendar"]);
    expect((await browse({ kind: "hamlet" })).titles).toEqual(["Launch Crew"]);
    await request(h.app).get("/v1/listings").query({ kind: "robot" }).expect(400);
  });

  it("needs every query word, case-insensitively, in any searched field", async () => {
    expect((await browse({ q: "GITHUB" })).titles).toEqual(["PR Reviewer", "Doc Writer"]);
    expect((await browse({ q: "github docs" })).titles).toEqual(["Doc Writer"]);
    expect((await browse({ q: "time" })).titles).toEqual(["Calendar"]);
    expect((await browse({ q: "business" })).titles).toEqual(["Launch Crew"]);
    expect((await browse({ q: "BOB-builder" })).titles).toEqual(["Launch Crew", "Doc Writer"]);
    expect((await browse({ q: "nothing-matches" })).total).toBe(0);
  });

  it("orders by rating, price and recency, ties by title", async () => {
    expect((await browse({ order: "rated" })).titles).toEqual([
      "Calendar",
      "Doc Writer",
      "PR Reviewer",
      "Launch Crew",
    ]);
    expect((await browse({ order: "cheapest" })).titles).toEqual([
      "Launch Crew",
      "Calendar",
      "PR Reviewer",
      "Doc Writer",
    ]);
    expect((await browse({ order: "newest" })).titles).toEqual([
      "PR Reviewer",
      "Doc Writer",
      "Calendar",
      "Launch Crew",
    ]);
    await request(h.app).get("/v1/listings").query({ order: "random" }).expect(400);
  });

  it("pages with limit and offset", async () => {
    const page = await browse({ order: "cheapest", limit: 2, offset: 1 });
    expect(page.titles).toEqual(["Calendar", "PR Reviewer"]);
    expect(page.total).toBe(4);
    await request(h.app).get("/v1/listings").query({ limit: 0 }).expect(400);
    await request(h.app).get("/v1/listings").query({ limit: 101 }).expect(400);
    await request(h.app).get("/v1/listings").query({ offset: -1 }).expect(400);
  });

  it("returns full listing objects", async () => {
    const { items } = await browse({ q: "reviewer" });
    expect(items[0]).toMatchObject({
      id: ids.reviewer,
      kind: "agent",
      priceCents: 900,
      currency: "usd",
      status: "listed",
      seller: { login: "alice", verified: true, name: "alice name" },
      latest: { version: "1.1.0" },
      owners: 1,
      stars: 4,
      reviews: 2,
    });
  });
});

describe("listing visibility", () => {
  function detail(id: string, token?: string) {
    const call = request(h.app).get(`/v1/listings/${id}`);
    return token ? call.set(auth(token)) : call;
  }

  it("shows a listed listing to anyone", async () => {
    const response = await detail(ids.reviewer!).expect(200);
    expect(response.body).toMatchObject({ owned: false, mine: false, reviews: [] });
    expect(response.body.versions).toHaveLength(2);
    const mine = await detail(ids.reviewer!, alice).expect(200);
    expect(mine.body.mine).toBe(true);
  });

  it("hides drafts and unlisted listings from strangers but not the seller or admins", async () => {
    for (const id of [ids.draft!, ids.unlisted!]) {
      await detail(id).expect(404);
      await detail(id, carol).expect(404);
      await detail(id, alice).expect(200);
      await detail(id, admin).expect(200);
    }
  });

  it("shows a removed listing only to its seller and admins", async () => {
    const response = await detail(ids.removed!).expect(404);
    expect(response.body.error.code).toBe("not_found");
    await detail(ids.removed!, carol).expect(404);
    const seller = await detail(ids.removed!, alice).expect(200);
    expect(seller.body.listing.status).toBe("removed");
    await detail(ids.removed!, admin).expect(200);
  });

  it("answers 404 for an unknown listing", async () => {
    await detail("l_unknown").expect(404);
  });

  it("lists the seller's own listings in every status, newest first", async () => {
    const response = await request(h.app).get("/v1/me/listings").set(auth(alice)).expect(200);
    const titles = (response.body.items as { title: string }[]).map((item) => item.title);
    expect(titles).toEqual([
      "Removed Agent",
      "Unlisted Agent",
      "Empty",
      "Draft Agent",
      "Calendar",
      "PR Reviewer",
    ]);
  });
});
