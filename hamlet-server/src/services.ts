import type { PrismaClient } from "./generated/prisma/client.ts";

/** The part of an R2 bucket the server uses. An `R2Bucket` satisfies it. */
export interface BlobStore {
  get(
    key: string,
    options?: { range?: { offset: number; length: number } },
  ): Promise<{ arrayBuffer(): Promise<ArrayBuffer> } | null>;
  put(
    key: string,
    value: ArrayBuffer | Uint8Array,
    options?: { httpMetadata?: { contentType?: string } },
  ): Promise<unknown>;
  delete(key: string): Promise<void>;
}

export interface CheckoutRequest {
  purchaseId: string;
  listingId: string;
  buyerId: string;
  title: string;
  amountCents: number;
  feeCents: number;
  destinationAccountId: string;
  successUrl: string;
  cancelUrl: string;
}

/** An official asset, paid to the platform's own account: no Connect. */
export interface PlatformCheckoutRequest {
  assetPurchaseId: string;
  assetId: string;
  buyerId: string;
  title: string;
  amountCents: number;
  successUrl: string;
  cancelUrl: string;
}

/**
 * A verified Stripe webhook event, reduced to what the server acts on.
 * A checkout carries `assetPurchaseId` for an official asset and
 * `purchaseId` for a listing.
 */
export type PaymentEvent =
  | {
      kind: "checkout_paid" | "checkout_failed";
      sessionId: string;
      purchaseId: string | null;
      assetPurchaseId: string | null;
    }
  | { kind: "account_updated"; accountId: string; chargesEnabled: boolean }
  | { kind: "ignored"; type: string };

/** The Stripe calls the server makes. */
export interface Payments {
  createAccount(input: { userId: string; login: string }): Promise<{ id: string }>;
  createAccountLink(input: {
    accountId: string;
    refreshUrl: string;
    returnUrl: string;
  }): Promise<{ url: string }>;
  retrieveAccount(accountId: string): Promise<{ id: string; chargesEnabled: boolean }>;
  createCheckoutSession(input: CheckoutRequest): Promise<{ id: string; url: string }>;
  createPlatformCheckoutSession(
    input: PlatformCheckoutRequest,
  ): Promise<{ id: string; url: string }>;
  /** Throws when the signature does not match the body. */
  verifyWebhookEvent(rawBody: Uint8Array, signature: string): Promise<PaymentEvent>;
}

export interface DeviceStart {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  interval: number;
  expiresIn: number;
}

export type DevicePoll =
  | { status: "approved"; accessToken: string }
  | { status: "pending" | "slow_down" | "expired" | "denied" | "bad_code" };

export interface GitHubUser {
  id: string;
  login: string;
  name: string;
  avatarUrl: string;
}

/** The GitHub device flow and user lookup. */
export interface GitHub {
  startDevice(): Promise<DeviceStart>;
  pollDevice(deviceCode: string): Promise<DevicePoll>;
  getUser(accessToken: string): Promise<GitHubUser>;
}

export interface Config {
  platformFeePercent: number;
  maxPackageBytes: number;
  /** Without a trailing slash. */
  publicUrl: string;
  /** Lowercased GitHub logins. */
  adminLogins: string[];
}

export interface Services {
  prisma: PrismaClient;
  blobs: BlobStore;
  payments: Payments;
  github: GitHub;
  config: Config;
  now(): Date;
}
