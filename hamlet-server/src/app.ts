import express, { type Express } from "express";
import { authenticate } from "./auth.ts";
import { jsonBody } from "./body.ts";
import { errorHandler, notFoundHandler } from "./errors.ts";
import { adminRoutes } from "./routes/admin.ts";
import { authRoutes } from "./routes/auth.ts";
import { browseRoutes } from "./routes/browse.ts";
import { downloadRoutes } from "./routes/download.ts";
import { meRoutes } from "./routes/me.ts";
import { pageRoutes } from "./routes/pages.ts";
import { purchaseRoutes } from "./routes/purchases.ts";
import { reviewRoutes } from "./routes/reviews.ts";
import { sellRoutes } from "./routes/sell.ts";
import { webhookRoutes } from "./routes/webhook.ts";
import type { Services } from "./services.ts";

/** The whole HTTP API. Everything it touches comes in through `services`. */
export function createApp(services: Services): Express {
  const app = express();
  app.disable("x-powered-by");

  app.get("/health", (_req, res) => {
    res.json({ ok: true });
  });

  // Before the JSON parser: the webhook is verified over its raw bytes.
  app.use("/v1/stripe", webhookRoutes(services));

  app.use(jsonBody(services.config));
  app.use(authenticate(services));

  app.use(pageRoutes());
  app.use("/v1/auth", authRoutes(services));
  app.use("/v1/me", meRoutes(services));
  app.use("/v1/listings", browseRoutes(services));
  app.use("/v1/listings", sellRoutes(services));
  app.use("/v1/listings", downloadRoutes(services));
  app.use("/v1/listings", reviewRoutes(services));
  app.use("/v1", purchaseRoutes(services));
  app.use("/v1/admin", adminRoutes(services));

  app.use(notFoundHandler);
  app.use(errorHandler);
  return app;
}
