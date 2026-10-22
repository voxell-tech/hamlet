import request from "supertest";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { listedListing } from "./support/flows.ts";
import { type Harness, auth, createHarness } from "./support/harness.ts";

let h: Harness;
let seller: string;
let admin: string;
beforeAll(async () => {
  h = await createHarness();
  seller = await h.signIn("seller");
  admin = await h.signIn("admin");
});
afterAll(async () => {
  await h.dispose();
});

function review(listingId: string, token: string, body: object) {
  return request(h.app).post(`/v1/listings/${listingId}/reviews`).set(auth(token)).send(body);
}

async function owner(listingId: string, login: string) {
  const token = await h.signIn(login);
  await request(h.app).post(`/v1/listings/${listingId}/purchase`).set(auth(token)).expect(200);
  return token;
}

describe("reviews", () => {
  it("are for owners, one each, and feed the listing's stars", async () => {
    const listing = await listedListing(h, seller, { title: "Reviewed" });
    const ann = await owner(listing.id, "ann");
    const ben = await owner(listing.id, "ben");

    const first = await review(listing.id, ann, { stars: 2, words: "Meh." }).expect(200);
    expect(first.body).toEqual({
      by: { login: "ann", avatarUrl: "https://avatars.example/ann" },
      stars: 2,
      words: "Meh.",
      createdAt: expect.any(String),
    });
    await review(listing.id, ben, { stars: 5 }).expect(200);
    const replaced = await review(listing.id, ann, { stars: 4, words: "Better now." }).expect(200);
    expect(replaced.body.stars).toBe(4);

    const detail = await request(h.app).get(`/v1/listings/${listing.id}`).expect(200);
    expect(detail.body.listing).toMatchObject({ owners: 2, stars: 4.5, reviews: 2 });
    expect(detail.body.reviews).toEqual([
      expect.objectContaining({
        by: expect.objectContaining({ login: "ann" }),
        words: "Better now.",
      }),
      expect.objectContaining({ by: expect.objectContaining({ login: "ben" }), words: "" }),
    ]);
  });

  it("are refused to non-owners and the seller", async () => {
    const listing = await listedListing(h, seller, { title: "Guarded" });
    const stranger = await h.signIn("stranger");
    const refused = await review(listing.id, stranger, { stars: 5 }).expect(403);
    expect(refused.body.error.code).toBe("forbidden");
    await review(listing.id, seller, { stars: 5 }).expect(403);
    await request(h.app).post(`/v1/listings/${listing.id}/reviews`).send({ stars: 5 }).expect(401);
  });

  it("need 1 to 5 whole stars and at most 1000 chars", async () => {
    const listing = await listedListing(h, seller, { title: "Strict" });
    const cid = await owner(listing.id, "cid");
    for (const body of [
      { stars: 0 },
      { stars: 6 },
      { stars: 4.5 },
      { stars: 3, words: "x".repeat(1001) },
      {},
    ]) {
      await review(listing.id, cid, body).expect(400);
    }
  });
});

describe("admin", () => {
  it("takes a listing down and restores it as unlisted", async () => {
    const listing = await listedListing(h, seller, { title: "Disputed" });
    const takedown = await request(h.app)
      .post(`/v1/admin/listings/${listing.id}/takedown`)
      .set(auth(admin))
      .send({ reason: "copyright claim" })
      .expect(200);
    expect(takedown.body.status).toBe("removed");
    const stored = await h.prisma.listing.findUniqueOrThrow({ where: { id: listing.id } });
    expect(stored.removedReason).toBe("copyright claim");

    const browse = await request(h.app).get("/v1/listings").expect(200);
    expect(browse.body.items.map((item: { id: string }) => item.id)).not.toContain(listing.id);
    const edit = await request(h.app)
      .patch(`/v1/listings/${listing.id}`)
      .set(auth(seller))
      .send({ title: "Sneaky" })
      .expect(403);
    expect(edit.body.error.code).toBe("forbidden");
    await request(h.app).post(`/v1/listings/${listing.id}/publish`).set(auth(seller)).expect(403);

    const restored = await request(h.app)
      .post(`/v1/admin/listings/${listing.id}/restore`)
      .set(auth(admin))
      .expect(200);
    expect(restored.body.status).toBe("unlisted");
    await request(h.app)
      .post(`/v1/admin/listings/${listing.id}/restore`)
      .set(auth(admin))
      .expect(400);
    await request(h.app).post(`/v1/listings/${listing.id}/publish`).set(auth(seller)).expect(200);
  });

  it("is only for admins and needs a reason", async () => {
    const listing = await listedListing(h, seller, { title: "Safe" });
    const takedown = (token: string, body: object) =>
      request(h.app).post(`/v1/admin/listings/${listing.id}/takedown`).set(auth(token)).send(body);
    const refused = await takedown(seller, { reason: "mine" }).expect(403);
    expect(refused.body.error.code).toBe("forbidden");
    await request(h.app)
      .post(`/v1/admin/listings/${listing.id}/takedown`)
      .send({ reason: "x" })
      .expect(401);
    await takedown(admin, {}).expect(400);
    await request(h.app)
      .post("/v1/admin/listings/l_missing/takedown")
      .set(auth(admin))
      .send({ reason: "x" })
      .expect(404);
  });
});
