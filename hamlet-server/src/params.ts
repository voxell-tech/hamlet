import type { Request } from "express";
import { notFound } from "./errors.ts";

/** A single path parameter, e.g. the `:id` of `/v1/listings/:id`. */
export function param(req: Request, name: string): string {
  const value = req.params[name];
  if (typeof value !== "string") throw notFound();
  return value;
}
