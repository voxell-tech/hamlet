import { Router } from "express";
import { currentUser, requireAuth } from "../auth.ts";
import { badRequest, conflict, notFound, payoutsRequired } from "../errors.ts";
import type { User } from "../generated/prisma/client.ts";
import { newId } from "../ids.ts";
import { findListing, owns, refreshListingStats } from "../listings.ts";
import { platformFeeCents } from "../money.ts";
import { refreshPayouts } from "../payouts.ts";
import type { Services } from "../services.ts";
import type { ListingRow } from "../views.ts";
import { param } from "../params.ts";
import { settleFromStripe, settlePurchase } from "../settle.ts";

export function purchaseRoutes(services: Services): Router {
  const router = Router();

  router.post("/listings/:id/purchase", requireAuth, async (req, res) => {
    const buyer = currentUser(res);
    const listing = await findListing(services.prisma, param(req, "id"));
    if (!listing || listing.status !== "listed") throw notFound("no such listing for sale");
    if (listing.sellerId === buyer.id) throw badRequest("you cannot buy your own listing");
    if (await owns(services.prisma, listing.id, buyer.id)) throw conflict("you already own it");
    if (listing.priceCents === 0) {
      await claimFree(services, listing, buyer);
      res.json({ owned: true });
      return;
    }
    res.json({ owned: false, ...(await startCheckout(services, listing, buyer)) });
  });

  router.get("/purchases/:id", requireAuth, async (req, res) => {
    const id = param(req, "id");
    let purchase = await services.prisma.purchase.findUnique({ where: { id } });
    if (!purchase || purchase.buyerId !== currentUser(res).id) throw notFound("no such purchase");
    const pending = purchase;
    const settled = await settleFromStripe(services, pending, (outcome) =>
      settlePurchase(services, pending, outcome),
    );
    if (settled)
      purchase = (await services.prisma.purchase.findUnique({ where: { id } })) ?? purchase;
    res.json({ id: purchase.id, listingId: purchase.listingId, status: purchase.status });
  });

  return router;
}

async function claimFree(services: Services, listing: ListingRow, buyer: User): Promise<void> {
  const now = services.now();
  await services.prisma.purchase.create({
    data: {
      id: newId("p"),
      listingId: listing.id,
      buyerId: buyer.id,
      status: "paid",
      amountCents: 0,
      feeCents: 0,
      paidAt: now,
      createdAt: now,
      updatedAt: now,
    },
  });
  await refreshListingStats(services.prisma, listing.id);
}

async function startCheckout(services: Services, listing: ListingRow, buyer: User) {
  const seller = await refreshPayouts(services, listing.seller);
  if (!seller.stripeAccountId || !seller.payoutsEnabled) {
    throw payoutsRequired("the seller cannot take payments right now");
  }
  const now = services.now();
  const purchaseId = newId("p");
  const feeCents = platformFeeCents(listing.priceCents, services.config.platformFeePercent);
  await services.prisma.purchase.create({
    data: {
      id: purchaseId,
      listingId: listing.id,
      buyerId: buyer.id,
      status: "pending",
      amountCents: listing.priceCents,
      feeCents,
      createdAt: now,
      updatedAt: now,
    },
  });
  const session = await services.payments.createCheckoutSession({
    purchaseId,
    listingId: listing.id,
    buyerId: buyer.id,
    title: listing.title,
    amountCents: listing.priceCents,
    feeCents,
    destinationAccountId: seller.stripeAccountId,
    successUrl: `${services.config.publicUrl}/checkout/success`,
    cancelUrl: `${services.config.publicUrl}/checkout/cancel`,
  });
  await services.prisma.purchase.update({
    where: { id: purchaseId },
    data: { stripeSessionId: session.id },
  });
  return { purchaseId, checkoutUrl: session.url };
}
