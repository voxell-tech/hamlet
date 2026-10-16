import type { ErrorRequestHandler, RequestHandler } from "express";
import { ZodError } from "zod";

export type ErrorCode =
  | "bad_request"
  | "unauthorized"
  | "forbidden"
  | "not_found"
  | "conflict"
  | "too_large"
  | "payouts_required"
  | "pending"
  | "slow_down"
  | "expired"
  | "denied"
  | "internal";

export class HttpError extends Error {
  constructor(
    readonly status: number,
    readonly code: ErrorCode,
    message: string,
  ) {
    super(message);
  }
}

export const badRequest = (message: string) => new HttpError(400, "bad_request", message);
export const unauthorized = (message = "sign in first") =>
  new HttpError(401, "unauthorized", message);
export const forbidden = (message: string) => new HttpError(403, "forbidden", message);
export const notFound = (message = "not found") => new HttpError(404, "not_found", message);
export const conflict = (message: string) => new HttpError(409, "conflict", message);
export const tooLarge = (message: string) => new HttpError(413, "too_large", message);
export const payoutsRequired = (message: string) => new HttpError(403, "payouts_required", message);

/** Turns a zod error into one readable line, e.g. `title: Too big`. */
export function describeZodError(error: ZodError, prefix = ""): string {
  return error.issues
    .slice(0, 3)
    .map((issue) => {
      const path = [prefix, ...issue.path.map(String)].filter(Boolean).join(".");
      return path ? `${path}: ${issue.message}` : issue.message;
    })
    .join("; ");
}

export const notFoundHandler: RequestHandler = (req) => {
  throw notFound(`no route for ${req.method} ${req.path}`);
};

export const errorHandler: ErrorRequestHandler = (err: unknown, _req, res, next) => {
  if (res.headersSent) {
    next(err);
    return;
  }
  const httpError = toHttpError(err);
  if (httpError.status >= 500) console.error(err);
  res.status(httpError.status).json({
    error: { code: httpError.code, message: httpError.message },
  });
};

function toHttpError(err: unknown): HttpError {
  if (err instanceof HttpError) return err;
  if (err instanceof ZodError) return badRequest(describeZodError(err));
  const type = errorField(err, "type");
  if (type === "entity.too.large") return tooLarge("request body is too large");
  if (type === "entity.parse.failed") return badRequest("body is not valid JSON");
  if (type !== undefined && errorField(err, "status") === 415) {
    return badRequest("unsupported body encoding");
  }
  const prismaCode = errorField(err, "code");
  if (prismaCode === "P2002") return conflict("it already exists");
  if (prismaCode === "P2025") return notFound();
  return new HttpError(500, "internal", "something went wrong on the server");
}

function errorField(err: unknown, field: string): unknown {
  if (typeof err !== "object" || err === null) return undefined;
  return (err as Record<string, unknown>)[field];
}
