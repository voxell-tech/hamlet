import { Buffer } from "node:buffer";
import express, { Router } from "express";
import { currentUser, requireAuth } from "../auth.ts";
import { badRequest, notFound } from "../errors.ts";
import { newId } from "../ids.ts";
import { assertNotRemoved, sellersListing } from "../listings.ts";
import { tooLargePackage } from "../package/index.ts";
import type { Services } from "../services.ts";
import { param } from "../params.ts";

/**
 * The most bytes of one part of an upload. Hamlet's plugins cannot send
 * a whole package in one request, so they send it in parts this size.
 */
export const MAX_PART_BYTES = 1024 * 1024;

const UPLOAD_ID = /^up_[0-9a-f]{24}$/;

const partBody = express.raw({ type: () => true, limit: MAX_PART_BYTES });

/** A package sent in parts, before it becomes a version. */
export function uploadRoutes(services: Services): Router {
  const router = Router();
  const { prisma, config } = services;

  router.post("/:id/uploads", requireAuth, async (req, res) => {
    const listing = await sellersListing(prisma, config, param(req, "id"), currentUser(res));
    assertNotRemoved(listing);
    res.status(201).json({ id: newId("up"), partBytes: MAX_PART_BYTES });
  });

  router.post("/:id/uploads/:upload/parts/:index", requireAuth, partBody, async (req, res) => {
    const listing = await sellersListing(prisma, config, param(req, "id"), currentUser(res));
    assertNotRemoved(listing);
    const upload = uploadId(param(req, "upload"));
    const index = partIndex(param(req, "index"), config.maxPackageBytes);
    if (!Buffer.isBuffer(req.body) || req.body.length === 0) {
      throw badRequest("send the part's bytes as the body");
    }
    await services.blobs.put(partKey(listing.id, upload, index), req.body);
    res.status(204).end();
  });

  return router;
}

/** The parts `0..parts` of `upload` together, checked against the package limit. */
export async function assembleUpload(
  services: Services,
  listingId: string,
  upload: string,
  parts: number,
): Promise<Uint8Array> {
  const id = uploadId(upload);
  const most = services.config.maxPackageBytes;
  if (!Number.isInteger(parts) || parts < 1 || parts > maxParts(most)) {
    throw badRequest(`parts must be 1 to ${maxParts(most)}`);
  }
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (let index = 0; index < parts; index++) {
    const blob = await services.blobs.get(partKey(listingId, id, index));
    if (!blob) throw badRequest(`part ${index} of the upload is missing`);
    const chunk = new Uint8Array(await blob.arrayBuffer());
    size += chunk.byteLength;
    if (size > most) throw tooLargePackage(size, most);
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

/** Forgets the parts of an upload once its version is made. */
export async function dropUpload(
  services: Services,
  listingId: string,
  upload: string,
  parts: number,
): Promise<void> {
  for (let index = 0; index < parts; index++) {
    await services.blobs.delete(partKey(listingId, upload, index));
  }
}

function uploadId(value: string): string {
  if (!UPLOAD_ID.test(value)) throw notFound("no such upload");
  return value;
}

function partIndex(value: string, maxPackageBytes: number): number {
  const index = /^\d{1,4}$/.test(value) ? Number(value) : -1;
  if (index < 0 || index >= maxParts(maxPackageBytes)) {
    throw badRequest(`a part's index is 0 to ${maxParts(maxPackageBytes) - 1}`);
  }
  return index;
}

/** Enough parts for a package at the limit sent in parts of any size. */
function maxParts(maxPackageBytes: number): number {
  return Math.min(1024, Math.max(1, Math.ceil(maxPackageBytes / 1024)));
}

function partKey(listingId: string, upload: string, index: number): string {
  return `uploads/${listingId}/${upload}/${index}`;
}
