import Stripe from "stripe";
import { createStripeWebhookVerifier } from "../../src/stripe.ts";
import type {
  CheckoutRequest,
  DevicePoll,
  DeviceStart,
  GitHub,
  GitHubUser,
  Payments,
  PlatformCheckoutRequest,
} from "../../src/services.ts";

export const WEBHOOK_SECRET = "whsec_test_secret_for_tests_only";
export const CONNECT_WEBHOOK_SECRET = "whsec_test_connect_secret_for_tests_only";

/** Stripe without the network. Webhooks go through the real verifier. */
export class FakePayments implements Payments {
  readonly accounts = new Map<string, { chargesEnabled: boolean }>();
  readonly checkouts: (CheckoutRequest & { sessionId: string })[] = [];
  readonly platformCheckouts: (PlatformCheckoutRequest & { sessionId: string })[] = [];
  readonly links: { accountId: string; refreshUrl: string; returnUrl: string }[] = [];
  private counter = 0;
  readonly verifyWebhookEvent = createStripeWebhookVerifier(
    `${WEBHOOK_SECRET}, ${CONNECT_WEBHOOK_SECRET}`,
  );

  async createAccount(): Promise<{ id: string }> {
    const id = `acct_test_${++this.counter}`;
    this.accounts.set(id, { chargesEnabled: false });
    return { id };
  }

  async createAccountLink(input: { accountId: string; refreshUrl: string; returnUrl: string }) {
    this.links.push(input);
    return { url: `https://connect.stripe.com/setup/e/${input.accountId}` };
  }

  async retrieveAccount(accountId: string) {
    const account = this.accounts.get(accountId);
    if (!account) throw new Error(`no account ${accountId}`);
    return { id: accountId, chargesEnabled: account.chargesEnabled };
  }

  async createCheckoutSession(request: CheckoutRequest) {
    const sessionId = `cs_test_${++this.counter}`;
    this.checkouts.push({ ...request, sessionId });
    return { id: sessionId, url: `https://checkout.stripe.com/c/pay/${sessionId}` };
  }

  async createPlatformCheckoutSession(request: PlatformCheckoutRequest) {
    const sessionId = `cs_test_${++this.counter}`;
    this.platformCheckouts.push({ ...request, sessionId });
    return { id: sessionId, url: `https://checkout.stripe.com/c/pay/${sessionId}` };
  }

  enable(accountId: string): void {
    this.accounts.set(accountId, { chargesEnabled: true });
  }
}

/** Signs a webhook body the way Stripe does. */
export function signWebhook(
  payload: string,
  options: { secret?: string; timestamp?: number } = {},
): string {
  return Stripe.webhooks.generateTestHeaderString({
    payload,
    secret: options.secret ?? WEBHOOK_SECRET,
    ...(options.timestamp === undefined ? {} : { timestamp: options.timestamp }),
  });
}

export function stripeEvent(type: string, object: Record<string, unknown>): string {
  return JSON.stringify({
    id: `evt_${Math.random().toString(36).slice(2)}`,
    object: "event",
    type,
    data: { object },
  });
}

/** GitHub's device flow, scripted: each device code answers from a queue. */
export class FakeGitHub implements GitHub {
  private counter = 0;
  private readonly answers = new Map<string, DevicePoll["status"][]>();
  private readonly approvals = new Map<string, GitHubUser>();

  async startDevice(): Promise<DeviceStart> {
    const deviceCode = `device_${++this.counter}`;
    return {
      deviceCode,
      userCode: `CODE-${String(this.counter).padStart(4, "0")}`,
      verificationUri: "https://github.com/login/device",
      interval: 5,
      expiresIn: 900,
    };
  }

  /** The next polls of `deviceCode` answer these, then `approved` as `user`. */
  script(deviceCode: string, answers: DevicePoll["status"][], user?: GitHubUser): void {
    this.answers.set(deviceCode, [...answers]);
    if (user) this.approvals.set(deviceCode, user);
  }

  async pollDevice(deviceCode: string): Promise<DevicePoll> {
    const queued = this.answers.get(deviceCode)?.shift();
    if (queued && queued !== "approved") return { status: queued };
    const user = this.approvals.get(deviceCode);
    if (!user) return { status: queued ? "bad_code" : "pending" };
    return { status: "approved", accessToken: `gho_${deviceCode}` };
  }

  async getUser(accessToken: string): Promise<GitHubUser> {
    const user = this.approvals.get(accessToken.replace(/^gho_/, ""));
    if (!user) throw new Error("unknown token");
    return user;
  }
}
