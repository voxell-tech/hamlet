import express, { type RequestHandler } from "express";
import type { Config } from "./services.ts";

declare global {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace Express {
    interface Request {
      /** The raw body of a package upload, kept to store the package as sent. */
      rawBody?: Uint8Array;
    }
  }
}

export const COVER_TYPES = ["image/png", "image/jpeg", "image/webp"];
export const MAX_COVER_BYTES = 2 * 1024 * 1024;
// Room for `version` and `changes` around a package at the size limit.
const UPLOAD_ENVELOPE_BYTES = 64 * 1024;
const SMALL_JSON_LIMIT = "100kb";
const VERSION_UPLOAD_PATH = /^\/v1\/listings\/[^/]+\/versions\/?$/;

/** JSON bodies: small by default, package-sized for version uploads. */
export function jsonBody(config: Config): RequestHandler {
  const small = express.json({ limit: SMALL_JSON_LIMIT });
  const upload = express.json({
    limit: config.maxPackageBytes + UPLOAD_ENVELOPE_BYTES,
    verify: (req, _res, buf) => {
      (req as express.Request).rawBody = buf;
    },
  });
  return (req, res, next) => {
    const isUpload = req.method === "POST" && VERSION_UPLOAD_PATH.test(req.path);
    return (isUpload ? upload : small)(req, res, next);
  };
}

/** The webhook signature is over the exact bytes, so they stay raw. */
export const webhookBody = express.raw({ type: () => true, limit: "1mb" });

export const coverBody = express.raw({ type: COVER_TYPES, limit: MAX_COVER_BYTES });
