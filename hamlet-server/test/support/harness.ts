import { readFileSync } from "node:fs";
import { join } from "node:path";
import { PrismaD1 } from "@prisma/adapter-d1";
import type { Express } from "express";
import { Miniflare } from "miniflare";
import request from "supertest";
import { DOWN_DIR, UP_DIR, migrationNames, sqlStatements } from "../../scripts/migrationFiles.ts";
import { createApp } from "../../src/app.ts";
import { DEFAULT_CONFIG } from "../../src/config.ts";
import { PrismaClient } from "../../src/generated/prisma/client.ts";
import type { BlobStore, Config, Services } from "../../src/services.ts";
import { FakeGitHub, FakePayments } from "./fakes.ts";

type D1 = Awaited<ReturnType<Miniflare["getD1Database"]>>;

export interface Harness {
  app: Express;
  prisma: PrismaClient;
  db: D1;
  payments: FakePayments;
  github: FakeGitHub;
  services: Services;
  /** Signs in through the device flow; returns the session token. */
  signIn(login: string): Promise<string>;
  /** Signs in and connects payouts that can take payments. */
  signInSeller(login: string): Promise<string>;
  dispose(): Promise<void>;
}

/** A fresh in-memory D1 and R2 from Miniflare, with every migration applied. */
export async function createHarness(config: Partial<Config> = {}): Promise<Harness> {
  const miniflare = new Miniflare({
    modules: true,
    script: "export default {}",
    d1Databases: ["DB"],
    r2Buckets: ["BLOBS"],
  });
  const db = await miniflare.getD1Database("DB");
  const blobs = await miniflare.getR2Bucket("BLOBS");
  await applyMigrations(db, "up");

  const prisma = new PrismaClient({ adapter: new PrismaD1(db as never) });
  const payments = new FakePayments();
  const github = new FakeGitHub();
  // Every reading of the clock is a second later, so times are distinct.
  let clock = Date.parse("2026-01-01T00:00:00Z");
  const services: Services = {
    prisma,
    blobs: blobs as unknown as BlobStore,
    payments,
    github,
    config: { ...DEFAULT_CONFIG, adminLogins: ["admin"], ...config },
    now: () => new Date((clock += 1000)),
  };
  const app = createApp(services);
  let githubIds = 0;

  async function signIn(login: string): Promise<string> {
    const start = await request(app).post("/v1/auth/device").expect(200);
    const deviceCode = start.body.deviceCode as string;
    github.script(deviceCode, [], {
      id: String(++githubIds),
      login,
      name: `${login} name`,
      avatarUrl: `https://avatars.example/${login}`,
    });
    const poll = await request(app).post("/v1/auth/device/poll").send({ deviceCode }).expect(200);
    return poll.body.token as string;
  }

  async function signInSeller(login: string): Promise<string> {
    const token = await signIn(login);
    await request(app).post("/v1/me/payouts").set(auth(token)).expect(200);
    const user = await prisma.user.findFirstOrThrow({ where: { login } });
    payments.enable(user.stripeAccountId!);
    await request(app).get("/v1/me").set(auth(token)).expect(200);
    return token;
  }

  return {
    app,
    prisma,
    db,
    payments,
    github,
    services,
    signIn,
    signInSeller,
    dispose: async () => {
      await prisma.$disconnect();
      await miniflare.dispose();
    },
  };
}

export function auth(token: string): { Authorization: string } {
  return { Authorization: `Bearer ${token}` };
}

/** Runs every migration up (oldest first) or down (newest first). */
export async function applyMigrations(db: D1, direction: "up" | "down"): Promise<void> {
  const names = direction === "up" ? migrationNames() : migrationNames().reverse();
  for (const name of names) {
    const dir = direction === "up" ? UP_DIR : DOWN_DIR;
    const sql = readFileSync(join(dir, name), "utf8");
    await db.batch(sqlStatements(sql).map((statement) => db.prepare(statement)));
  }
}

export async function startMiniflareD1(): Promise<{ db: D1; dispose(): Promise<void> }> {
  const miniflare = new Miniflare({
    modules: true,
    script: "export default {}",
    d1Databases: ["DB"],
  });
  return { db: await miniflare.getD1Database("DB"), dispose: () => miniflare.dispose() };
}
