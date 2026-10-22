import { readFileSync } from "node:fs";
import { join } from "node:path";
import request from "supertest";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { ASSET_FIXTURE_DIR, fixtureCatalog, publishAssets } from "./support/assets.ts";
import { signWebhook, stripeEvent } from "./support/fakes.ts";
import { listedListing } from "./support/flows.ts";
import { type Harness, auth, createHarness } from "./support/harness.ts";

let h: Harness;
beforeAll(async () => {
  h = await createHarness();
  await publishAssets(h, fixtureCatalog());
});
afterAll(async () => {
  await h.dispose();
});

let buyers = 0;
const newBuyer = () => h.signIn(`asset-buyer-${++buyers}`);

const fixtureFile = (name: string) => readFileSync(join(ASSET_FIXTURE_DIR, "files", name));

function purchase(assetId: string, token: string) {
  return request(h.app).post(`/v1/assets/${assetId}/purchase`).set(auth(token));
}

function download(assetId: string, token: string) {
  return request(h.app)
    .get(`/v1/assets/${assetId}/file`)
    .set(auth(token))
    .buffer(true)
    .parse((res, done) => {
      const chunks: Buffer[] = [];
      res.on("data", (chunk: Buffer) => chunks.push(chunk));
      res.on("end", () => done(null, Buffer.concat(chunks)));
    });
}

function sendWebhook(payload: string) {
  return request(h.app)
    .post("/v1/stripe/webhook")
    .set("Content-Type", "application/json")
    .set("Stripe-Signature", signWebhook(payload))
    .send(payload);
}

function assetCheckoutEvent(type: string, assetPurchaseId: string, sessionId: string) {
  return stripeEvent(type, {
    id: sessionId,
    object: "checkout.session",
    payment_status: "paid",
    metadata: { assetPurchaseId },
  });
}

async function buyPaid(assetId: string, buyer: string) {
  const response = await purchase(assetId, buyer).expect(200);
  const checkout = h.payments.platformCheckouts.at(-1)!;
  return { body: response.body, checkout };
}

describe("the asset catalog", () => {
  it("lists every listed asset by target, category, price and title", async () => {
    const response = await request(h.app).get("/v1/assets").expect(200);
    expect(response.body.items.map((asset: { id: string }) => asset.id)).toEqual([
      "agent-emote-wave",
      "agent-skin-sakura",
      "hamlet-mesh-well",
    ]);
    expect(response.body.items[1]).toEqual({
      id: "agent-skin-sakura",
      target: "agent",
      category: "skin",
      title: "Sakura",
      summary: "Soft cherry-blossom pinks for your agent.",
      priceCents: 99,
      currency: "usd",
      version: "1.0.0",
      frames: 2,
      previewUrl: "/v1/assets/agent-skin-sakura/preview/{frame}?v=1.0.0",
      owned: false,
      updatedAt: expect.stringMatching(/^\d{4}-\d\d-\d\dT/),
    });
  });

  it("filters by target and category, and refuses unknown ones", async () => {
    const agents = await request(h.app).get("/v1/assets?target=agent").expect(200);
    expect(agents.body.items.map((asset: { id: string }) => asset.id)).toEqual([
      "agent-emote-wave",
      "agent-skin-sakura",
    ]);
    const meshes = await request(h.app).get("/v1/assets?target=hamlet&category=mesh").expect(200);
    expect(meshes.body.items.map((asset: { id: string }) => asset.id)).toEqual([
      "hamlet-mesh-well",
    ]);
    const none = await request(h.app).get("/v1/assets?target=agent&category=mesh").expect(200);
    expect(none.body.items).toEqual([]);
    await request(h.app).get("/v1/assets?target=plugin").expect(400);
  });

  it("shows one asset, and 404 for an unknown one", async () => {
    const response = await request(h.app).get("/v1/assets/hamlet-mesh-well").expect(200);
    expect(response.body).toMatchObject({ id: "hamlet-mesh-well", priceCents: 0, owned: false });
    await request(h.app).get("/v1/assets/no-such-asset").expect(404);
  });
});

describe("preview frames", () => {
  it("are public, cacheable PNGs, numbered from 0", async () => {
    const response = await request(h.app).get("/v1/assets/agent-skin-sakura/preview/1").expect(200);
    expect(response.headers["content-type"]).toBe("image/png");
    expect(response.headers["cache-control"]).toBe("public, max-age=86400");
    expect(Buffer.from(response.body)).toEqual(
      readFileSync(join(ASSET_FIXTURE_DIR, "previews", "agent-skin-sakura", "1.png")),
    );
  });

  it("are 404 out of range, for an unknown asset and for none at all", async () => {
    await request(h.app).get("/v1/assets/agent-skin-sakura/preview/2").expect(404);
    await request(h.app).get("/v1/assets/agent-skin-sakura/preview/-1").expect(404);
    await request(h.app).get("/v1/assets/agent-skin-sakura/preview/one").expect(404);
    await request(h.app).get("/v1/assets/agent-emote-wave/preview/0").expect(404);
    await request(h.app).get("/v1/assets/no-such-asset/preview/0").expect(404);
  });
});

describe("free assets", () => {
  it("are owned once claimed, with no Stripe, and only once", async () => {
    const buyer = await newBuyer();
    await request(h.app).post("/v1/assets/hamlet-mesh-well/purchase").expect(401);
    const checkoutsBefore = h.payments.platformCheckouts.length;
    await purchase("hamlet-mesh-well", buyer).expect(200, { owned: true });
    expect(h.payments.platformCheckouts).toHaveLength(checkoutsBefore);

    const detail = await request(h.app)
      .get("/v1/assets/hamlet-mesh-well")
      .set(auth(buyer))
      .expect(200);
    expect(detail.body.owned).toBe(true);
    const again = await purchase("hamlet-mesh-well", buyer).expect(409);
    expect(again.body.error.code).toBe("conflict");

    const file = await download("hamlet-mesh-well", buyer).expect(200);
    expect(file.headers["content-type"]).toBe("model/gltf-binary");
    expect(file.body).toEqual(fixtureFile("hamlet-mesh-well.glb"));
  });
});

describe("paid assets", () => {
  it("open a Checkout Session on the platform's own account", async () => {
    const buyer = await newBuyer();
    const { body, checkout } = await buyPaid("agent-skin-sakura", buyer);
    expect(body).toEqual({
      owned: false,
      purchaseId: expect.stringMatching(/^ap_/),
      checkoutUrl: expect.stringMatching(/^https:\/\/checkout\.stripe\.com\//),
    });
    expect(checkout).toMatchObject({
      assetPurchaseId: body.purchaseId,
      assetId: "agent-skin-sakura",
      title: "Sakura",
      amountCents: 99,
      successUrl: "http://localhost:8787/checkout/success",
      cancelUrl: "http://localhost:8787/checkout/cancel",
    });
    expect(checkout).not.toHaveProperty("destinationAccountId");
    expect(checkout).not.toHaveProperty("feeCents");

    const status = await request(h.app)
      .get(`/v1/asset-purchases/${body.purchaseId}`)
      .set(auth(buyer))
      .expect(200);
    expect(status.body).toEqual({
      id: body.purchaseId,
      assetId: "agent-skin-sakura",
      status: "pending",
    });
    const stranger = await newBuyer();
    await request(h.app)
      .get(`/v1/asset-purchases/${body.purchaseId}`)
      .set(auth(stranger))
      .expect(404);
    await download("agent-skin-sakura", buyer).expect(403);
  });

  it("are owned once Stripe says paid when the buyer asks, with no webhook", async () => {
    const buyer = await newBuyer();
    const { body, checkout } = await buyPaid("agent-skin-sakura", buyer);
    const asked = () =>
      request(h.app).get(`/v1/asset-purchases/${body.purchaseId}`).set(auth(buyer)).expect(200);
    expect((await asked()).body.status).toBe("pending");
    h.payments.outcomes.set(checkout.sessionId, "paid");
    expect((await asked()).body.status).toBe("paid");
    await download("agent-skin-sakura", buyer).expect(200);
    // A checkout Stripe calls expired is not undone once paid.
    h.payments.outcomes.set(checkout.sessionId, "failed");
    expect((await asked()).body.status).toBe("paid");
  });

  it("are owned once the webhook says paid, and only then downloadable", async () => {
    const buyer = await newBuyer();
    const stranger = await newBuyer();
    const { body, checkout } = await buyPaid("agent-skin-sakura", buyer);
    const paid = assetCheckoutEvent(
      "checkout.session.completed",
      body.purchaseId,
      checkout.sessionId,
    );
    await sendWebhook(paid).expect(200);
    await sendWebhook(paid).expect(200);

    const status = await request(h.app)
      .get(`/v1/asset-purchases/${body.purchaseId}`)
      .set(auth(buyer))
      .expect(200);
    expect(status.body.status).toBe("paid");
    const catalog = await request(h.app).get("/v1/assets").set(auth(buyer)).expect(200);
    const owned = catalog.body.items.filter((asset: { owned: boolean }) => asset.owned);
    expect(owned.map((asset: { id: string }) => asset.id)).toEqual(["agent-skin-sakura"]);

    const file = await download("agent-skin-sakura", buyer).expect(200);
    expect(file.headers["content-type"]).toBe("application/json");
    expect(file.headers["cache-control"]).toBe("private, no-store");
    expect(file.headers["x-asset-version"]).toBe("1.0.0");
    expect(file.body).toEqual(fixtureFile("agent-skin-sakura.json"));

    const refused = await download("agent-skin-sakura", stranger).expect(403);
    expect(JSON.parse(refused.body.toString()).error.code).toBe("forbidden");
    await request(h.app).get("/v1/assets/agent-skin-sakura/file").expect(401);
    await purchase("agent-skin-sakura", buyer).expect(409);
  });

  it("expire when Checkout expires, but never undo a paid one", async () => {
    const buyer = await newBuyer();
    const expired = await buyPaid("agent-skin-sakura", buyer);
    await sendWebhook(
      assetCheckoutEvent(
        "checkout.session.expired",
        expired.body.purchaseId,
        expired.checkout.sessionId,
      ),
    ).expect(200);
    const status = await request(h.app)
      .get(`/v1/asset-purchases/${expired.body.purchaseId}`)
      .set(auth(buyer))
      .expect(200);
    expect(status.body.status).toBe("expired");

    const paid = await buyPaid("agent-skin-sakura", buyer);
    await sendWebhook(
      assetCheckoutEvent(
        "checkout.session.async_payment_succeeded",
        paid.body.purchaseId,
        paid.checkout.sessionId,
      ),
    ).expect(200);
    await sendWebhook(
      assetCheckoutEvent(
        "checkout.session.async_payment_failed",
        paid.body.purchaseId,
        paid.checkout.sessionId,
      ),
    ).expect(200);
    const after = await request(h.app)
      .get(`/v1/asset-purchases/${paid.body.purchaseId}`)
      .set(auth(buyer))
      .expect(200);
    expect(after.body.status).toBe("paid");
  });

  it("leave listing purchases to the listing webhook", async () => {
    const seller = await h.signInSeller("asset-test-seller");
    const listing = await listedListing(h, seller, { title: "Still Sold", priceCents: 900 });
    const buyer = await newBuyer();
    const response = await request(h.app)
      .post(`/v1/listings/${listing.id}/purchase`)
      .set(auth(buyer))
      .expect(200);
    const checkout = h.payments.checkouts.at(-1)!;
    await sendWebhook(
      stripeEvent("checkout.session.completed", {
        id: checkout.sessionId,
        object: "checkout.session",
        payment_status: "paid",
        metadata: { purchaseId: response.body.purchaseId },
      }),
    ).expect(200);
    const status = await request(h.app)
      .get(`/v1/purchases/${response.body.purchaseId}`)
      .set(auth(buyer))
      .expect(200);
    expect(status.body.status).toBe("paid");
    const mine = await request(h.app).get("/v1/me/assets").set(auth(buyer)).expect(200);
    expect(mine.body.items).toEqual([]);
  });
});

describe("unlisted assets", () => {
  it("leave the catalog but stay with their owners", async () => {
    const owner = await newBuyer();
    const stranger = await newBuyer();
    await purchase("agent-emote-wave", owner).expect(200, { owned: true });
    await publishAssets(
      h,
      fixtureCatalog().filter((entry) => entry.id !== "agent-emote-wave"),
    );

    const catalog = await request(h.app).get("/v1/assets").expect(200);
    expect(catalog.body.items.map((asset: { id: string }) => asset.id)).not.toContain(
      "agent-emote-wave",
    );
    await request(h.app).get("/v1/assets/agent-emote-wave").expect(404);
    await purchase("agent-emote-wave", stranger).expect(404);
    await download("agent-emote-wave", stranger).expect(403);

    const file = await download("agent-emote-wave", owner).expect(200);
    expect(file.body).toEqual(fixtureFile("agent-emote-wave.json"));
    const mine = await request(h.app).get("/v1/me/assets").set(auth(owner)).expect(200);
    expect(mine.body.items).toEqual([
      expect.objectContaining({ id: "agent-emote-wave", owned: true }),
    ]);

    await publishAssets(h, fixtureCatalog());
    await request(h.app).get("/v1/assets/agent-emote-wave").expect(200);
  });
});

describe("my assets", () => {
  it("are every owned asset, owned: true, and need auth", async () => {
    const buyer = await newBuyer();
    await request(h.app).get("/v1/me/assets").expect(401);
    const empty = await request(h.app).get("/v1/me/assets").set(auth(buyer)).expect(200);
    expect(empty.body).toEqual({ items: [] });

    await purchase("hamlet-mesh-well", buyer).expect(200);
    await purchase("agent-emote-wave", buyer).expect(200);
    await buyPaid("agent-skin-sakura", buyer);
    const mine = await request(h.app).get("/v1/me/assets").set(auth(buyer)).expect(200);
    expect(
      mine.body.items.map((asset: { id: string; owned: boolean }) => [asset.id, asset.owned]),
    ).toEqual([
      ["agent-emote-wave", true],
      ["hamlet-mesh-well", true],
    ]);
  });
});
