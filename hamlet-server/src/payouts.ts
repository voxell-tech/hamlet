import type { User } from "./generated/prisma/client.ts";
import type { Services } from "./services.ts";

/**
 * Whether the user can take payments. A connected account that could not
 * yet is checked with Stripe again, as the webhook may not have come.
 */
export async function refreshPayouts(services: Services, user: User): Promise<User> {
  if (!user.stripeAccountId || user.payoutsEnabled) return user;
  const account = await services.payments.retrieveAccount(user.stripeAccountId);
  if (!account.chargesEnabled) return user;
  return services.prisma.user.update({
    where: { id: user.id },
    data: { payoutsEnabled: true },
  });
}

export async function ensureStripeAccount(services: Services, user: User): Promise<User> {
  if (user.stripeAccountId) return user;
  const account = await services.payments.createAccount({ userId: user.id, login: user.login });
  return services.prisma.user.update({
    where: { id: user.id },
    data: { stripeAccountId: account.id },
  });
}
