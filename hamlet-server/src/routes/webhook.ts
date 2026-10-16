import { Buffer } from "node:buffer";
import { Router } from "express";
import { webhookBody } from "../body.ts";
import { HttpError, badRequest } from "../errors.ts";
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
    case "checkout_paid": {
      const purchase = await findPurchase(services, event.purchaseId, event.sessionId);
      if (!purchase || purchase.status === "paid") return;
      const now = services.now();
      await prisma.purchase.update({
        where: { id: purchase.id },
        data: { status: "paid", paidAt: now, updatedAt: now },
      });
      await refreshListingStats(prisma, purchase.listingId);
      return;
    }
    case "checkout_failed": {
      const purchase = await findPurchase(services, event.purchaseId, event.sessionId);
      if (!purchase) return;
      await prisma.purchase.updateMany({
        where: { id: purchase.id, status: "pending" },
        data: { status: "expired", updatedAt: services.now() },
      });
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

async function findPurchase(services: Services, purchaseId: string | null, sessionId: string) {
  const { purchase } = services.prisma;
  if (purchaseId) {
    const found = await purchase.findUnique({ where: { id: purchaseId } });
    if (found) return found;
  }
  return purchase.findUnique({ where: { stripeSessionId: sessionId } });
}
