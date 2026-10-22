import { badRequest, describeZodError, tooLarge } from "../errors.ts";
import { type PackageMeta, deriveMeta } from "./meta.ts";
import { rawMemberBytes } from "./rawMember.ts";
import { packageSchema } from "./schema.ts";

export interface UploadedPackage {
  /** The package exactly as sent, to store and serve back. */
  bytes: Uint8Array;
  meta: PackageMeta;
}

/**
 * Checks the `package` member of an upload body and derives its meta.
 * `rawBody` is the whole request body; `parsed` is its parsed `package`.
 */
export function readUploadedPackage(
  rawBody: Uint8Array | undefined,
  parsed: unknown,
  listingKind: string,
  maxBytes: number,
): UploadedPackage {
  if (parsed === undefined) throw badRequest("package is missing");
  const bytes =
    (rawBody && rawMemberBytes(rawBody, "package")) ??
    new TextEncoder().encode(JSON.stringify(parsed));
  return checkPackage(bytes, parsed, listingKind, maxBytes);
}

/** Checks a package uploaded in parts, `bytes` being the whole of it. */
export function readPackageBytes(
  bytes: Uint8Array,
  listingKind: string,
  maxBytes: number,
): UploadedPackage {
  if (bytes.byteLength > maxBytes) throw tooLargePackage(bytes.byteLength, maxBytes);
  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    throw badRequest("package is not JSON");
  }
  return checkPackage(bytes, parsed, listingKind, maxBytes);
}

export function tooLargePackage(size: number, maxBytes: number) {
  return tooLarge(`package is ${size} bytes; the limit is ${maxBytes}`);
}

function checkPackage(
  bytes: Uint8Array,
  parsed: unknown,
  listingKind: string,
  maxBytes: number,
): UploadedPackage {
  if (bytes.byteLength > maxBytes) throw tooLargePackage(bytes.byteLength, maxBytes);
  const result = packageSchema.safeParse(parsed);
  if (!result.success) throw badRequest(describeZodError(result.error, "package"));
  if (result.data.kind !== listingKind) {
    throw badRequest(`package kind is ${result.data.kind} but the listing is a ${listingKind}`);
  }
  return { bytes, meta: deriveMeta(result.data) };
}
