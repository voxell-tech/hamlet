import { Router } from "express";
import type { PrismaClient, User } from "../generated/prisma/client.ts";
import { ASSET_ORDER, assetJson } from "../assets.ts";
import { currentUser, requireAuth } from "../auth.ts";
import { LISTING_INCLUDE } from "../listings.ts";
import { ensureStripeAccount, refreshPayouts } from "../payouts.ts";
import type { Services } from "../services.ts";
import { NEWEST_VERSION_FIRST } from "../versionNumber.ts";
import { listingJson, meJson, versionJson } from "../views.ts";

export function meRoutes(services: Services): Router {
  const router = Router();
  router.use(requireAuth);

  router.get("/", async (_req, res) => {
    const user = await refreshPayouts(services, currentUser(res));
    res.json(meJson(user));
  });

  router.post("/payouts", async (_req, res) => {
    const user = await ensureStripeAccount(services, currentUser(res));
    const { url } = await services.payments.createAccountLink({
      accountId: user.stripeAccountId!,
      refreshUrl: `${services.config.publicUrl}/payouts/refresh`,
      returnUrl: `${services.config.publicUrl}/payouts/return`,
    });
    res.json({ url });
  });

  router.get("/listings", async (_req, res) => {
    const listings = await services.prisma.listing.findMany({
      where: { sellerId: currentUser(res).id },
      orderBy: [{ createdAt: "desc" }, { id: "asc" }],
      include: LISTING_INCLUDE,
    });
    res.json({ items: listings.map(listingJson) });
  });

  router.get("/library", async (_req, res) => {
    res.json({ items: await library(services.prisma, currentUser(res)) });
  });

  // Owned assets, listed or not.
  router.get("/assets", async (_req, res) => {
    const assets = await services.prisma.asset.findMany({
      where: { purchases: { some: { buyerId: currentUser(res).id, status: "paid" } } },
      orderBy: ASSET_ORDER,
    });
    res.json({ items: assets.map((asset) => assetJson(asset, true)) });
  });

  return router;
}

async function library(prisma: PrismaClient, user: User) {
  const purchases = await prisma.purchase.findMany({
    where: { buyerId: user.id, status: "paid", listing: { status: { not: "removed" } } },
    orderBy: [{ paidAt: "asc" }, { createdAt: "asc" }],
    include: {
      listing: { include: { seller: true, versions: { orderBy: NEWEST_VERSION_FIRST } } },
    },
  });
  const firstPurchase = new Map<string, (typeof purchases)[number]>();
  for (const purchase of purchases) {
    if (!firstPurchase.has(purchase.listingId)) firstPurchase.set(purchase.listingId, purchase);
  }
  return [...firstPurchase.values()]
    .map((purchase) => ({
      listing: listingJson(purchase.listing),
      versions: purchase.listing.versions.map(versionJson),
      ownedSince: (purchase.paidAt ?? purchase.createdAt).toISOString(),
    }))
    .sort((a, b) => b.ownedSince.localeCompare(a.ownedSince));
}
