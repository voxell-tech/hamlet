import type { User } from "./generated/prisma/client.ts";
import type { Services } from "./services.ts";

/**
 * The start of the account of a seller paid through the platform's own
 * Stripe account, as the mock sellers of `pnpm db:seed` are, each
 * `acct_staff_<login>` as an account is one user's: their sales are taken
 * with no transfer, so the whole amount stays with the platform.
 */
export const STAFF_ACCOUNT = "acct_staff_";

/** Whether `account` is paid through the platform's own account. */
export function isStaffAccount(account: string): boolean {
  return account.startsWith(STAFF_ACCOUNT);
}

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
