import { Buffer } from "node:buffer";
import { Router } from "express";
import { webhookBody } from "../body.ts";
import { HttpError, badRequest } from "../errors.ts";
import type { AssetPurchase, Purchase } from "../generated/prisma/client.ts";
import { refreshListingStats } from "../listings.ts";
import type { PaymentEvent, Services } from "../services.ts";

export function webhookRoutes(services: Services): Router {
  const router = Router();

  router.post("/webhook", webhookBody, async (req, res) => {
    const signature = req.header("stripe-signature");
    if (!signature || !Buffer.isBuffer(req.body)) throw badRequest("missing Stripe signature");
    let event: PaymentEvent;
    try {
      event = await services.payments.verifyWebhookEvent(req.body, signature);
    } catch (error) {
      if (error instanceof HttpError) throw error;
      throw badRequest("the Stripe signature does not match");
    }
    await handleEvent(services, event);
    res.json({ received: true });
  });

  return router;
}

/** Every branch is idempotent: Stripe may send an event more than once. */
async function handleEvent(services: Services, event: PaymentEvent): Promise<void> {
  const { prisma } = services;
  switch (event.kind) {
    case "checkout_paid":
    case "checkout_failed": {
      const assetPurchase = await findAssetPurchase(services, event);
      if (assetPurchase) {
        await settleAssetPurchase(services, assetPurchase, event.kind);
        return;
      }
      if (event.assetPurchaseId) return;
      const purchase = await findPurchase(services, event.purchaseId, event.sessionId);
      if (purchase) await settlePurchase(services, purchase, event.kind);
      return;
    }
    case "account_updated":
      await prisma.user.updateMany({
        where: { stripeAccountId: event.accountId },
        data: { payoutsEnabled: event.chargesEnabled },
      });
      return;
    case "ignored":
      return;
  }
}

type CheckoutOutcome = "checkout_paid" | "checkout_failed";

async function settlePurchase(
  services: Services,
  purchase: Purchase,
  outcome: CheckoutOutcome,
): Promise<void> {
  const { prisma } = services;
  if (outcome === "checkout_failed") {
    await prisma.purchase.updateMany({
      where: { id: purchase.id, status: "pending" },
      data: { status: "expired", updatedAt: services.now() },
    });
    return;
  }
  if (purchase.status === "paid") return;
  const now = services.now();
  await prisma.purchase.update({
    where: { id: purchase.id },
    data: { status: "paid", paidAt: now, updatedAt: now },
  });
  await refreshListingStats(prisma, purchase.listingId);
}

async function settleAssetPurchase(
  services: Services,
  purchase: AssetPurchase,
  outcome: CheckoutOutcome,
): Promise<void> {
  const { assetPurchase } = services.prisma;
  if (outcome === "checkout_failed") {
    await assetPurchase.updateMany({
      where: { id: purchase.id, status: "pending" },
      data: { status: "expired", updatedAt: services.now() },
    });
    return;
  }
  if (purchase.status === "paid") return;
  const now = services.now();
  await assetPurchase.update({
    where: { id: purchase.id },
    data: { status: "paid", paidAt: now, updatedAt: now },
  });
}

async function findPurchase(services: Services, purchaseId: string | null, sessionId: string) {
  const { purchase } = services.prisma;
  if (purchaseId) {
    const found = await purchase.findUnique({ where: { id: purchaseId } });
    if (found) return found;
  }
  return purchase.findUnique({ where: { stripeSessionId: sessionId } });
}

/** By the metadata, else by the session for an event that lost it. */
async function findAssetPurchase(
  services: Services,
  event: { assetPurchaseId: string | null; purchaseId: string | null; sessionId: string },
) {
  const { assetPurchase } = services.prisma;
  if (event.assetPurchaseId) {
    const found = await assetPurchase.findUnique({ where: { id: event.assetPurchaseId } });
    if (found) return found;
  }
  if (event.purchaseId) return null;
  return assetPurchase.findUnique({ where: { stripeSessionId: event.sessionId } });
}
