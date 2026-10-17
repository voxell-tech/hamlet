import type { AssetPurchase, Purchase } from "./generated/prisma/client.ts";
import { refreshListingStats } from "./listings.ts";
import type { Services } from "./services.ts";

/**
 * Marks a purchase paid or expired, as Stripe said of its checkout.
 * Idempotent: the webhook and a buyer asking may both say so.
 */
export type CheckoutOutcome = "checkout_paid" | "checkout_failed";

export async function settlePurchase(
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

export async function settleAssetPurchase(
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

/**
 * A pending purchase, settled from Stripe's own word on its checkout
 * when the webhook has not come: one late, or none at all where no
 * `stripe listen` forwards them. Stripe being out of reach leaves it
 * pending.
 */
export async function settleFromStripe<
  T extends { status: string; stripeSessionId: string | null },
>(
  services: Services,
  purchase: T,
  settle: (outcome: CheckoutOutcome) => Promise<void>,
): Promise<boolean> {
  if (purchase.status !== "pending" || !purchase.stripeSessionId) return false;
  let state;
  try {
    state = await services.payments.checkoutOutcome(purchase.stripeSessionId);
  } catch {
    return false;
  }
  if (state === "open") return false;
  await settle(state === "paid" ? "checkout_paid" : "checkout_failed");
  return true;
}
