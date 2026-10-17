import request from "supertest";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { listedListing } from "./support/flows.ts";
import { CONNECT_WEBHOOK_SECRET, signWebhook, stripeEvent } from "./support/fakes.ts";
import { type Harness, auth, createHarness } from "./support/harness.ts";

let h: Harness;
let seller: string;
let admin: string;
let sellerAccount: string;
beforeAll(async () => {
  h = await createHarness();
  seller = await h.signInSeller("seller");
  admin = await h.signIn("admin");
  sellerAccount = (await h.prisma.user.findFirstOrThrow({ where: { login: "seller" } }))
    .stripeAccountId!;
});
afterAll(async () => {
  await h.dispose();
});

let buyers = 0;
const newBuyer = () => h.signIn(`buyer-${++buyers}`);

function purchase(listingId: string, token: string) {
  return request(h.app).post(`/v1/listings/${listingId}/purchase`).set(auth(token));
}

function download(listingId: string, token: string, version = "1.0.0") {
  return request(h.app)
    .get(`/v1/listings/${listingId}/versions/${version}/package`)
    .set(auth(token));
}

function sendWebhook(payload: string, signature = signWebhook(payload)) {
  return request(h.app)
    .post("/v1/stripe/webhook")
    .set("Content-Type", "application/json")
    .set("Stripe-Signature", signature)
    .send(payload);
}

function checkoutEvent(
  type: string,
  purchaseId: string,
  sessionId: string,
  paymentStatus = "paid",
) {
  return stripeEvent(type, {
    id: sessionId,
    object: "checkout.session",
    payment_status: paymentStatus,
    metadata: { purchaseId },
  });
}

async function startPaidPurchase(listingId: string, buyer: string) {
  const response = await purchase(listingId, buyer).expect(200);
  const checkout = h.payments.checkouts.at(-1)!;
  return { body: response.body, checkout };
}

describe("free listings", () => {
  it("are owned at once with no Stripe, and count as owners", async () => {
    const listing = await listedListing(h, seller, { title: "Free One", priceCents: 0 });
    const buyer = await newBuyer();
    const checkoutsBefore = h.payments.checkouts.length;
    await purchase(listing.id, buyer).expect(200, { owned: true });
    expect(h.payments.checkouts).toHaveLength(checkoutsBefore);

    const detail = await request(h.app)
      .get(`/v1/listings/${listing.id}`)
      .set(auth(buyer))
      .expect(200);
    expect(detail.body).toMatchObject({ owned: true, mine: false, listing: { owners: 1 } });
    await download(listing.id, buyer).expect(200);

    const again = await purchase(listing.id, buyer).expect(409);
    expect(again.body.error.code).toBe("conflict");
  });
});

describe("paid listings", () => {
  it("open a Checkout Session with the platform fee and the seller as destination", async () => {
    const listing = await listedListing(h, seller, { title: "Paid One", priceCents: 1005 });
    const buyer = await newBuyer();
    const { body, checkout } = await startPaidPurchase(listing.id, buyer);
    expect(body).toEqual({
      owned: false,
      purchaseId: expect.stringMatching(/^p_/),
      checkoutUrl: expect.stringMatching(/^https:\/\/checkout\.stripe\.com\//),
    });
    expect(checkout).toMatchObject({
      purchaseId: body.purchaseId,
      title: "Paid One",
      amountCents: 1005,
      feeCents: 101, // 100.5 rounded
      destinationAccountId: sellerAccount,
      successUrl: "http://localhost:8787/checkout/success",
      cancelUrl: "http://localhost:8787/checkout/cancel",
    });
    const status = await request(h.app)
      .get(`/v1/purchases/${body.purchaseId}`)
      .set(auth(buyer))
      .expect(200);
    expect(status.body).toEqual({ id: body.purchaseId, listingId: listing.id, status: "pending" });
    const other = await newBuyer();
    await request(h.app).get(`/v1/purchases/${body.purchaseId}`).set(auth(other)).expect(404);
  });

  it("settle from Stripe when the buyer asks and no webhook came", async () => {
    const listing = await listedListing(h, seller, { title: "Paid Asked", priceCents: 700 });
    const buyer = await newBuyer();
    const paid = await startPaidPurchase(listing.id, buyer);
    const asked = (id: string) =>
      request(h.app).get(`/v1/purchases/${id}`).set(auth(buyer)).expect(200);
    h.payments.outcomes.set(paid.checkout.sessionId, "paid");
    expect((await asked(paid.body.purchaseId)).body.status).toBe("paid");
    await download(listing.id, buyer).expect(200);
    const detail = await request(h.app).get(`/v1/listings/${listing.id}`).expect(200);
    expect(detail.body.listing.owners).toBe(1);

    const other = await newBuyer();
    const gone = await request(h.app)
      .post(`/v1/listings/${listing.id}/purchase`)
      .set(auth(other))
      .expect(200);
    h.payments.outcomes.set(h.payments.checkouts.at(-1)!.sessionId, "failed");
    const status = await request(h.app)
      .get(`/v1/purchases/${gone.body.purchaseId}`)
      .set(auth(other))
      .expect(200);
    expect(status.body.status).toBe("expired");
  });

  it("are owned once the webhook says paid, idempotently", async () => {
    const listing = await listedListing(h, seller, { title: "Paid Two", priceCents: 900 });
    const buyer = await newBuyer();
    const { body, checkout } = await startPaidPurchase(listing.id, buyer);
    expect(checkout.feeCents).toBe(90);
    await download(listing.id, buyer).expect(403);

    const paid = checkoutEvent("checkout.session.completed", body.purchaseId, checkout.sessionId);
    await sendWebhook(paid).expect(200);
    await sendWebhook(paid).expect(200);

    const status = await request(h.app)
      .get(`/v1/purchases/${body.purchaseId}`)
      .set(auth(buyer))
      .expect(200);
    expect(status.body.status).toBe("paid");
    const detail = await request(h.app)
      .get(`/v1/listings/${listing.id}`)
      .set(auth(buyer))
      .expect(200);
    expect(detail.body).toMatchObject({ owned: true, listing: { owners: 1 } });
    await download(listing.id, buyer).expect(200);
    await purchase(listing.id, buyer).expect(409);
  });

  it("make a new session when bought again while pending, and either one can pay", async () => {
    const listing = await listedListing(h, seller, { title: "Paid Three", priceCents: 900 });
    const buyer = await newBuyer();
    const first = await startPaidPurchase(listing.id, buyer);
    const second = await startPaidPurchase(listing.id, buyer);
    expect(second.body.purchaseId).not.toBe(first.body.purchaseId);
    await sendWebhook(
      checkoutEvent(
        "checkout.session.async_payment_succeeded",
        first.body.purchaseId,
        first.checkout.sessionId,
      ),
    ).expect(200);
    const detail = await request(h.app)
      .get(`/v1/listings/${listing.id}`)
      .set(auth(buyer))
      .expect(200);
    expect(detail.body.owned).toBe(true);
  });

  it("do not count a completed session that is not paid yet", async () => {
    const listing = await listedListing(h, seller, { title: "Paid Four", priceCents: 900 });
    const buyer = await newBuyer();
    const { body, checkout } = await startPaidPurchase(listing.id, buyer);
    await sendWebhook(
      checkoutEvent("checkout.session.completed", body.purchaseId, checkout.sessionId, "unpaid"),
    ).expect(200);
    const status = await request(h.app)
      .get(`/v1/purchases/${body.purchaseId}`)
      .set(auth(buyer))
      .expect(200);
    expect(status.body.status).toBe("pending");
  });

  it("expire when Checkout expires or the payment fails, but never undo a paid one", async () => {
    const listing = await listedListing(h, seller, { title: "Paid Five", priceCents: 900 });
    const buyer = await newBuyer();
    const expired = await startPaidPurchase(listing.id, buyer);
    await sendWebhook(
      checkoutEvent(
        "checkout.session.expired",
        expired.body.purchaseId,
        expired.checkout.sessionId,
      ),
    ).expect(200);
    const status = await request(h.app)
      .get(`/v1/purchases/${expired.body.purchaseId}`)
      .set(auth(buyer))
      .expect(200);
    expect(status.body.status).toBe("expired");

    const paid = await startPaidPurchase(listing.id, buyer);
    await sendWebhook(
      checkoutEvent("checkout.session.completed", paid.body.purchaseId, paid.checkout.sessionId),
    ).expect(200);
    await sendWebhook(
      checkoutEvent(
        "checkout.session.async_payment_failed",
        paid.body.purchaseId,
        paid.checkout.sessionId,
      ),
    ).expect(200);
    const after = await request(h.app)
      .get(`/v1/purchases/${paid.body.purchaseId}`)
      .set(auth(buyer))
      .expect(200);
    expect(after.body.status).toBe("paid");
  });

  it("need the seller to be able to take payments", async () => {
    const listing = await listedListing(h, seller, { title: "Paid Six", priceCents: 900 });
    const sellerUser = await h.prisma.user.findFirstOrThrow({ where: { login: "seller" } });
    await h.prisma.user.update({ where: { id: sellerUser.id }, data: { payoutsEnabled: false } });
    h.payments.accounts.set(sellerAccount, { chargesEnabled: false });
    const response = await purchase(listing.id, await newBuyer()).expect(403);
    expect(response.body.error.code).toBe("payouts_required");
    h.payments.enable(sellerAccount);
    await purchase(listing.id, await newBuyer()).expect(200);
  });
});

describe("buying rules", () => {
  it("refuses one's own listing and anything not listed", async () => {
    const listing = await listedListing(h, seller, { title: "Rules" });
    const own = await purchase(listing.id, seller).expect(400);
    expect(own.body.error.code).toBe("bad_request");
    await request(h.app).post(`/v1/listings/${listing.id}/unlist`).set(auth(seller)).expect(200);
    await purchase(listing.id, await newBuyer()).expect(404);
    await purchase("l_missing", await newBuyer()).expect(404);
    await request(h.app).post(`/v1/listings/${listing.id}/purchase`).expect(401);
  });
});

describe("downloads and ownership", () => {
  it("are for the seller and owners; strangers get 403 on a listed listing", async () => {
    const listing = await listedListing(h, seller, { title: "Owned" });
    const owner = await newBuyer();
    const stranger = await newBuyer();
    await purchase(listing.id, owner).expect(200);
    await download(listing.id, seller).expect(200);
    await download(listing.id, owner).expect(200);
    const refused = await download(listing.id, stranger).expect(403);
    expect(refused.body.error.code).toBe("forbidden");
    await download(listing.id, owner, "9.9.9").expect(404);
    await download(listing.id, owner).set("Authorization", "").expect(401);
  });

  it("survive unlisting: owners keep the listing in their library and can download", async () => {
    const listing = await listedListing(h, seller, { title: "Kept" });
    const owner = await newBuyer();
    const stranger = await newBuyer();
    await purchase(listing.id, owner).expect(200);
    await request(h.app).post(`/v1/listings/${listing.id}/unlist`).set(auth(seller)).expect(200);

    await request(h.app).get(`/v1/listings/${listing.id}`).set(auth(owner)).expect(200);
    await download(listing.id, owner).expect(200);
    await download(listing.id, stranger).expect(404);
    await request(h.app).get(`/v1/listings/${listing.id}`).set(auth(stranger)).expect(404);

    const library = await request(h.app).get("/v1/me/library").set(auth(owner)).expect(200);
    expect(library.body.items).toEqual([
      {
        listing: expect.objectContaining({ id: listing.id, status: "unlisted" }),
        versions: [expect.objectContaining({ version: "1.0.0" })],
        ownedSince: expect.any(String),
      },
    ]);
  });

  it("stop when an admin takes the listing down, except for admins", async () => {
    const listing = await listedListing(h, seller, { title: "Taken Down" });
    const owner = await newBuyer();
    await purchase(listing.id, owner).expect(200);
    await request(h.app)
      .post(`/v1/admin/listings/${listing.id}/takedown`)
      .set(auth(admin))
      .send({ reason: "malware" })
      .expect(200);

    await download(listing.id, owner).expect(404);
    const sellerDownload = await download(listing.id, seller).expect(403);
    expect(sellerDownload.body.error.code).toBe("forbidden");
    await download(listing.id, admin).expect(200);
    const library = await request(h.app).get("/v1/me/library").set(auth(owner)).expect(200);
    expect(library.body.items).toEqual([]);
  });
});

describe("the Stripe webhook", () => {
  it("rejects a bad signature, another secret and an old timestamp", async () => {
    const payload = stripeEvent("account.updated", { id: sellerAccount, charges_enabled: false });
    const missing = await request(h.app)
      .post("/v1/stripe/webhook")
      .set("Content-Type", "application/json")
      .send(payload)
      .expect(400);
    expect(missing.body.error.code).toBe("bad_request");
    await sendWebhook(payload, "t=1,v1=deadbeef").expect(400);
    await sendWebhook(payload, signWebhook(payload, { secret: "whsec_other" })).expect(400);
    const old = Math.floor(Date.now() / 1000) - 3600;
    await sendWebhook(payload, signWebhook(payload, { timestamp: old })).expect(400);
    const tampered = signWebhook(payload);
    await sendWebhook(payload.replace("false", "true"), tampered).expect(400);
  });

  it("answers 200 to unknown events and purchases", async () => {
    await sendWebhook(stripeEvent("customer.created", { id: "cus_1" })).expect(200);
    await sendWebhook(
      checkoutEvent("checkout.session.completed", "p_unknown", "cs_unknown"),
    ).expect(200);
  });

  it("keeps payoutsEnabled in step with account.updated", async () => {
    const other = await h.signIn("other-seller");
    await request(h.app).post("/v1/me/payouts").set(auth(other)).expect(200);
    const user = await h.prisma.user.findFirstOrThrow({ where: { login: "other-seller" } });
    await sendWebhook(
      stripeEvent("account.updated", { id: user.stripeAccountId, charges_enabled: true }),
    ).expect(200);
    const me = await request(h.app).get("/v1/me").set(auth(other)).expect(200);
    expect(me.body.payouts).toEqual({ connected: true, enabled: true });
    // Connected-account events come signed with the Connect endpoint's secret.
    const disabled = stripeEvent("account.updated", {
      id: user.stripeAccountId,
      charges_enabled: false,
    });
    await sendWebhook(disabled, signWebhook(disabled, { secret: CONNECT_WEBHOOK_SECRET })).expect(
      200,
    );
    const after = await h.prisma.user.findUniqueOrThrow({ where: { id: user.id } });
    expect(after.payoutsEnabled).toBe(false);
  });
});
