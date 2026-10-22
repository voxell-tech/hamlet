import express, { type RequestHandler } from "express";
import { tooLarge } from "./errors.ts";
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
/**
 * A cover fits in one answer of the engine's fetch (512 KiB) and leaves room
 * in a plugin's call for another beside it (1 MiB a call).
 */
export const MAX_COVER_BYTES = 400 * 1024;
export const MAX_COVER_SIDE = 2048;
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

const rawCover = express.raw({ type: COVER_TYPES, limit: MAX_COVER_BYTES });

/** A cover's raw bytes, and a 413 that says the limit when there are too many. */
export const coverBody: RequestHandler = (req, res, next) => {
  rawCover(req, res, (err?: unknown) => {
    const type = typeof err === "object" && err !== null ? (err as { type?: unknown }).type : null;
    if (type === "entity.too.large") {
      next(tooLarge(`a cover is at most ${MAX_COVER_BYTES / 1024} KiB`));
      return;
    }
    next(err);
  });
};
