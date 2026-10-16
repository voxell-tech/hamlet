import { describe, expect, it } from "vitest";
import { createGitHub } from "../src/github.ts";
import { checkoutParams, toPaymentEvent } from "../src/stripe.ts";

type Call = { url: string; init: RequestInit | undefined };

function fakeFetch(answers: unknown[]): { fetcher: typeof fetch; calls: Call[] } {
  const calls: Call[] = [];
  const fetcher = (async (url: string | URL | Request, init?: RequestInit) => {
    calls.push({ url: String(url), init });
    return new Response(JSON.stringify(answers.shift()), {
      headers: { "Content-Type": "application/json" },
    });
  }) as typeof fetch;
  return { fetcher, calls };
}

describe("GitHub device flow client", () => {
  it("asks for a device code with the client id and read:user", async () => {
    const { fetcher, calls } = fakeFetch([
      {
        device_code: "dc",
        user_code: "ABCD-1234",
        verification_uri: "https://github.com/login/device",
        expires_in: 899,
        interval: 5,
      },
    ]);
    const start = await createGitHub("Iv1.client", fetcher).startDevice();
    expect(start).toEqual({
      deviceCode: "dc",
      userCode: "ABCD-1234",
      verificationUri: "https://github.com/login/device",
      interval: 5,
      expiresIn: 899,
    });
    expect(calls[0]!.url).toBe("https://github.com/login/device/code");
    expect(String(calls[0]!.init!.body)).toBe("client_id=Iv1.client&scope=read%3Auser");
    expect(new Headers(calls[0]!.init!.headers).get("Accept")).toBe("application/json");
  });

  it("maps GitHub's poll errors and reads the user", async () => {
    const { fetcher, calls } = fakeFetch([
      { error: "authorization_pending" },
      { error: "slow_down", interval: 10 },
      { error: "expired_token" },
      { error: "access_denied" },
      { access_token: "gho_x", token_type: "bearer" },
      { id: 42, login: "octo", name: null, avatar_url: "https://a/42" },
    ]);
    const github = createGitHub("Iv1.client", fetcher);
    const statuses = [];
    for (let i = 0; i < 5; i++) statuses.push(await github.pollDevice("dc"));
    expect(statuses).toEqual([
      { status: "pending" },
      { status: "slow_down" },
      { status: "expired" },
      { status: "denied" },
      { status: "approved", accessToken: "gho_x" },
    ]);
    expect(String(calls[0]!.init!.body)).toContain(
      "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code",
    );
    const user = await github.getUser("gho_x");
    expect(user).toEqual({ id: "42", login: "octo", name: "octo", avatarUrl: "https://a/42" });
    const headers = new Headers(calls[5]!.init!.headers);
    expect(headers.get("Authorization")).toBe("Bearer gho_x");
    expect(headers.get("User-Agent")).toBeTruthy();
  });

  it("fails clearly without a client id", async () => {
    const { fetcher } = fakeFetch([]);
    await expect(createGitHub(undefined, fetcher).startDevice()).rejects.toThrow(
      /GITHUB_CLIENT_ID/,
    );
  });
});

describe("Stripe requests", () => {
  it("builds a Checkout Session with the fee going to the platform and the rest to the seller", () => {
    const params = checkoutParams({
      purchaseId: "p_1",
      listingId: "l_1",
      buyerId: "u_1",
      title: "PR Reviewer",
      amountCents: 900,
      feeCents: 90,
      destinationAccountId: "acct_1",
      successUrl: "https://bazaar/checkout/success",
      cancelUrl: "https://bazaar/checkout/cancel",
    });
    expect(params).toMatchObject({
      mode: "payment",
      line_items: [
        {
          quantity: 1,
          price_data: { currency: "usd", unit_amount: 900, product_data: { name: "PR Reviewer" } },
        },
      ],
      payment_intent_data: {
        application_fee_amount: 90,
        transfer_data: { destination: "acct_1" },
      },
      metadata: { purchaseId: "p_1" },
      success_url: "https://bazaar/checkout/success",
      cancel_url: "https://bazaar/checkout/cancel",
    });
  });

  it("reduces Stripe events to what the server acts on", () => {
    const session = (status: string) => ({
      id: "cs_1",
      payment_status: status,
      metadata: { purchaseId: "p_1" },
    });
    const event = (type: string, object: object) =>
      toPaymentEvent({ type, data: { object } } as never);
    expect(event("checkout.session.completed", session("paid"))).toEqual({
      kind: "checkout_paid",
      sessionId: "cs_1",
      purchaseId: "p_1",
    });
    expect(event("checkout.session.completed", session("unpaid")).kind).toBe("ignored");
    expect(event("checkout.session.expired", session("unpaid")).kind).toBe("checkout_failed");
    expect(event("account.updated", { id: "acct_1", charges_enabled: true })).toEqual({
      kind: "account_updated",
      accountId: "acct_1",
      chargesEnabled: true,
    });
    expect(event("invoice.paid", {}).kind).toBe("ignored");
  });
});
