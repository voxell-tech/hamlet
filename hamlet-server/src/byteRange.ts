import { Buffer } from "node:buffer";
import type { Response } from "express";
import { badRequest } from "./errors.ts";
import type { BlobStore } from "./services.ts";

/** A part of a file, `length` bytes from `offset`. */
export interface ByteRange {
  offset: number;
  length: number;
}

/**
 * The bytes `offset` and `limit` ask for of a file of `size`, or null when
 * neither is given.
 */
export function byteRange(offset: unknown, limit: unknown, size: number): ByteRange | null {
  if (offset === undefined && limit === undefined) return null;
  const from = whole(offset ?? "0");
  const most = whole(limit ?? String(size));
  if (from === null || most === null || most < 1) {
    throw badRequest("offset and limit are whole numbers, limit at least 1");
  }
  if (from > size) throw badRequest(`offset is past the file's ${size} bytes`);
  return { offset: from, length: Math.min(most, size - from) };
}

/**
 * Answers `206` with `range` of the blob at `key`, of `size` bytes in all,
 * for clients that cannot take it whole. False when the blob is gone.
 */
export async function sendRange(
  res: Response,
  blobs: BlobStore,
  key: string,
  range: ByteRange,
  size: number,
): Promise<boolean> {
  const { offset, length } = range;
  let bytes = Buffer.alloc(0);
  if (length > 0) {
    const blob = await blobs.get(key, { range: { offset, length } });
    if (!blob) return false;
    bytes = Buffer.from(await blob.arrayBuffer());
  }
  res
    .status(206)
    .set("Content-Range", `bytes ${offset}-${offset + length - 1}/${size}`)
    .type("application/octet-stream")
    .send(bytes);
  return true;
}

function whole(value: unknown): number | null {
  return typeof value === "string" && /^\d{1,10}$/.test(value) ? Number(value) : null;
}
