import Stripe from "stripe";
import { HttpError } from "./errors.ts";
import type { CheckoutRequest, PaymentEvent, Payments } from "./services.ts";
import { CURRENCY } from "./money.ts";

export interface StripeSecrets {
  secretKey: string | undefined;
  webhookSecret: string | undefined;
}

/** `Payments` on the real Stripe API, using fetch so it runs on Workers. */
export function createStripePayments(secrets: StripeSecrets): Payments {
  let client: Stripe | undefined;
  const stripe = (): Stripe => {
    if (!secrets.secretKey) throw missingSecret("STRIPE_SECRET_KEY");
    client ??= new Stripe(secrets.secretKey, { httpClient: Stripe.createFetchHttpClient() });
    return client;
  };
  const verify = secrets.webhookSecret
    ? createStripeWebhookVerifier(secrets.webhookSecret)
    : () => Promise.reject(missingSecret("STRIPE_WEBHOOK_SECRET"));

  return {
    async createAccount({ userId, login }) {
      const account = await stripe().accounts.create({
        type: "express",
        metadata: { userId, login },
        capabilities: { card_payments: { requested: true }, transfers: { requested: true } },
      });
      return { id: account.id };
    },

    async createAccountLink({ accountId, refreshUrl, returnUrl }) {
      const link = await stripe().accountLinks.create({
        account: accountId,
        refresh_url: refreshUrl,
        return_url: returnUrl,
        type: "account_onboarding",
      });
      return { url: link.url };
    },

    async retrieveAccount(accountId) {
      const account = await stripe().accounts.retrieve(accountId);
      return { id: account.id, chargesEnabled: account.charges_enabled ?? false };
    },

    async createCheckoutSession(request) {
      const session = await stripe().checkout.sessions.create(checkoutParams(request), {
        idempotencyKey: request.purchaseId,
      });
      if (!session.url) throw new Error("Stripe returned a checkout session without a url");
      return { id: session.id, url: session.url };
    },

    verifyWebhookEvent: verify,
  };
}

export function checkoutParams(request: CheckoutRequest): Stripe.Checkout.SessionCreateParams {
  const metadata = {
    purchaseId: request.purchaseId,
    listingId: request.listingId,
    buyerId: request.buyerId,
  };
  return {
    mode: "payment",
    line_items: [
      {
        quantity: 1,
        price_data: {
          currency: CURRENCY,
          unit_amount: request.amountCents,
          product_data: { name: request.title },
        },
      },
    ],
    payment_intent_data: {
      application_fee_amount: request.feeCents,
      transfer_data: { destination: request.destinationAccountId },
      metadata,
    },
    client_reference_id: request.purchaseId,
    metadata,
    success_url: request.successUrl,
    cancel_url: request.cancelUrl,
  };
}

/**
 * Checks a webhook signature with Web Crypto (Workers has no Node HMAC
 * fast path) and reduces the event to what the server acts on.
 *
 * `webhookSecrets` may hold several comma-separated secrets: platform
 * events and connected-account events (`account.updated`) come from two
 * Stripe endpoints, each with its own signing secret.
 */
export function createStripeWebhookVerifier(
  webhookSecrets: string,
): (rawBody: Uint8Array, signature: string) => Promise<PaymentEvent> {
  const cryptoProvider = Stripe.createSubtleCryptoProvider();
  const secrets = webhookSecrets
    .split(",")
    .map((secret) => secret.trim())
    .filter(Boolean);
  return async (rawBody, signature) => {
    let lastError: unknown = new Error("no webhook secret");
    for (const secret of secrets) {
      try {
        const event = await Stripe.webhooks.constructEventAsync(
          rawBody,
          signature,
          secret,
          undefined,
          cryptoProvider,
        );
        return toPaymentEvent(event);
      } catch (error) {
        lastError = error;
      }
    }
    throw lastError;
  };
}

export function toPaymentEvent(event: Stripe.Event): PaymentEvent {
  switch (event.type) {
    case "checkout.session.completed":
      if (event.data.object.payment_status !== "paid") return { kind: "ignored", type: event.type };
      return checkoutEvent("checkout_paid", event.data.object);
    case "checkout.session.async_payment_succeeded":
      return checkoutEvent("checkout_paid", event.data.object);
    case "checkout.session.expired":
    case "checkout.session.async_payment_failed":
      return checkoutEvent("checkout_failed", event.data.object);
    case "account.updated":
      return {
        kind: "account_updated",
        accountId: event.data.object.id,
        chargesEnabled: event.data.object.charges_enabled ?? false,
      };
    default:
      return { kind: "ignored", type: event.type };
  }
}

function checkoutEvent(
  kind: "checkout_paid" | "checkout_failed",
  session: Stripe.Checkout.Session,
): PaymentEvent {
  return { kind, sessionId: session.id, purchaseId: session.metadata?.purchaseId ?? null };
}

function missingSecret(name: string): HttpError {
  return new HttpError(500, "internal", `${name} is not set on the server`);
}
