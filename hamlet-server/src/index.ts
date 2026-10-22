import { PrismaD1 } from "@prisma/adapter-d1";
import { httpServerHandler } from "cloudflare:node";
import { env } from "cloudflare:workers";
import { createApp } from "./app.ts";
import { parseConfig } from "./config.ts";
import { PrismaClient } from "./generated/prisma/client.ts";
import { createGitHub } from "./github.ts";
import type { Services } from "./services.ts";
import { createStripePayments } from "./stripe.ts";

// The port Express listens on inside the Worker; httpServerHandler routes
// incoming requests to it. It is not the port `wrangler dev` serves on.
const PORT = 3000;

// Secrets live in .dev.vars or `wrangler secret put`, not wrangler.jsonc,
// so the generated Env may not list them.
interface Secrets {
  STRIPE_SECRET_KEY?: string;
  STRIPE_WEBHOOK_SECRET?: string;
  GITHUB_CLIENT_ID?: string;
}
const secrets: Secrets = env as Secrets;

const services: Services = {
  prisma: new PrismaClient({ adapter: new PrismaD1(env.DB) }),
  blobs: env.BLOBS,
  payments: createStripePayments({
    secretKey: secrets.STRIPE_SECRET_KEY,
    webhookSecret: secrets.STRIPE_WEBHOOK_SECRET,
  }),
  github: createGitHub(secrets.GITHUB_CLIENT_ID),
  config: parseConfig(env),
  now: () => new Date(),
};

createApp(services).listen(PORT);

export default httpServerHandler({ port: PORT });
