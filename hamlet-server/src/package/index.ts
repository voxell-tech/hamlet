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
  if (bytes.byteLength > maxBytes) {
    throw tooLarge(`package is ${bytes.byteLength} bytes; the limit is ${maxBytes}`);
  }
  const result = packageSchema.safeParse(parsed);
  if (!result.success) throw badRequest(describeZodError(result.error, "package"));
  if (result.data.kind !== listingKind) {
    throw badRequest(`package kind is ${result.data.kind} but the listing is a ${listingKind}`);
  }
  return { bytes, meta: deriveMeta(result.data) };
}
