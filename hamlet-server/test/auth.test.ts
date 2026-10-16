import request from "supertest";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { sha256Hex } from "../src/ids.ts";
import { type Harness, auth, createHarness } from "./support/harness.ts";

let h: Harness;
beforeAll(async () => {
  h = await createHarness();
});
afterAll(async () => {
  await h.dispose();
});

const octocat = {
  id: "583231",
  login: "octocat",
  name: "The Octocat",
  avatarUrl: "https://avatars.githubusercontent.com/u/583231",
};

async function startDevice() {
  const response = await request(h.app).post("/v1/auth/device").expect(200);
  return response.body as { deviceCode: string };
}

function poll(deviceCode: string) {
  return request(h.app).post("/v1/auth/device/poll").send({ deviceCode });
}

describe("health", () => {
  it("answers ok", async () => {
    await request(h.app).get("/health").expect(200, { ok: true });
  });
});

describe("device sign-in", () => {
  it("starts a device flow", async () => {
    const response = await request(h.app).post("/v1/auth/device").expect(200);
    expect(response.body).toEqual({
      deviceCode: expect.any(String),
      userCode: expect.any(String),
      verificationUri: "https://github.com/login/device",
      interval: 5,
      expiresIn: 900,
    });
  });

  it("answers pending, slow_down, expired and denied with their codes", async () => {
    const { deviceCode } = await startDevice();
    h.github.script(deviceCode, ["pending", "slow_down", "expired", "denied"]);
    const pending = await poll(deviceCode).expect(428);
    expect(pending.body.error.code).toBe("pending");
    expect((await poll(deviceCode).expect(429)).body.error.code).toBe("slow_down");
    expect((await poll(deviceCode).expect(410)).body.error.code).toBe("expired");
    expect((await poll(deviceCode).expect(403)).body.error.code).toBe("denied");
  });

  it("rejects a poll without a device code", async () => {
    const response = await request(h.app).post("/v1/auth/device/poll").send({}).expect(400);
    expect(response.body.error.code).toBe("bad_request");
  });

  it("signs in once approved, upserting the user and storing only the token hash", async () => {
    const { deviceCode } = await startDevice();
    h.github.script(deviceCode, ["pending"], octocat);
    await poll(deviceCode).expect(428);
    const approved = await poll(deviceCode).expect(200);
    const { token, user } = approved.body;
    expect(token).toMatch(/^hms_[0-9a-f]{64}$/);
    expect(user).toEqual({
      id: expect.stringMatching(/^u_/),
      login: "octocat",
      name: "The Octocat",
      avatarUrl: octocat.avatarUrl,
    });

    const session = await h.prisma.session.findFirstOrThrow({ where: { userId: user.id } });
    expect(session.tokenHash).toBe(sha256Hex(token));
    const days = (session.expiresAt.getTime() - session.createdAt.getTime()) / 86_400_000;
    expect(days).toBe(90);

    const me = await request(h.app).get("/v1/me").set(auth(token)).expect(200);
    expect(me.body).toEqual({ ...user, payouts: { connected: false, enabled: false } });
  });

  it("refreshes login, name and avatar on the next sign-in", async () => {
    const { deviceCode } = await startDevice();
    h.github.script(deviceCode, [], { ...octocat, login: "octo-renamed", name: "Renamed" });
    const response = await poll(deviceCode).expect(200);
    expect(response.body.user.login).toBe("octo-renamed");
    expect(await h.prisma.user.count({ where: { githubId: octocat.id } })).toBe(1);
  });

  it("sign out revokes the token used", async () => {
    const token = await h.signIn("leaver");
    const other = await h.signIn("leaver");
    await request(h.app).delete("/v1/auth/session").set(auth(token)).expect(204);
    const after = await request(h.app).get("/v1/me").set(auth(token)).expect(401);
    expect(after.body.error.code).toBe("unauthorized");
    await request(h.app).get("/v1/me").set(auth(other)).expect(200);
  });

  it("answers 401 without a token or with a bad one", async () => {
    await request(h.app).get("/v1/me").expect(401);
    await request(h.app).get("/v1/me").set(auth("hms_nope")).expect(401);
    await request(h.app).delete("/v1/auth/session").expect(401);
  });
});

describe("payouts", () => {
  it("creates one Stripe account and returns an onboarding link", async () => {
    const token = await h.signIn("seller-one");
    const first = await request(h.app).post("/v1/me/payouts").set(auth(token)).expect(200);
    expect(first.body.url).toMatch(/^https:\/\/connect\.stripe\.com\//);
    await request(h.app).post("/v1/me/payouts").set(auth(token)).expect(200);
    const link = h.payments.links.at(-1)!;
    expect(link.refreshUrl).toBe("http://localhost:8787/payouts/refresh");
    expect(link.returnUrl).toBe("http://localhost:8787/payouts/return");
    const user = await h.prisma.user.findFirstOrThrow({ where: { login: "seller-one" } });
    expect(h.payments.links.filter((l) => l.accountId === user.stripeAccountId)).toHaveLength(2);

    const before = await request(h.app).get("/v1/me").set(auth(token)).expect(200);
    expect(before.body.payouts).toEqual({ connected: true, enabled: false });
    h.payments.enable(user.stripeAccountId!);
    const after = await request(h.app).get("/v1/me").set(auth(token)).expect(200);
    expect(after.body.payouts).toEqual({ connected: true, enabled: true });
  });
});

describe("pages and errors", () => {
  it("serves the browser pages", async () => {
    for (const path of [
      "/checkout/success",
      "/checkout/cancel",
      "/payouts/return",
      "/payouts/refresh",
    ]) {
      const response = await request(h.app).get(path).expect(200);
      expect(response.headers["content-type"]).toMatch(/text\/html/);
      expect(response.text).toContain("Hamlet");
    }
  });

  it("answers unknown routes and bad JSON in the error shape", async () => {
    const missing = await request(h.app).get("/v1/nothing").expect(404);
    expect(missing.body).toEqual({ error: { code: "not_found", message: expect.any(String) } });
    const bad = await request(h.app)
      .post("/v1/auth/device/poll")
      .set("Content-Type", "application/json")
      .send("{not json")
      .expect(400);
    expect(bad.body.error.code).toBe("bad_request");
  });
});
