import { Buffer } from "node:buffer";
import { Router } from "express";
import { currentUser, requireAuth } from "../auth.ts";
import { badRequest, forbidden, notFound } from "../errors.ts";
import { visibleListing } from "../listings.ts";
import type { Services } from "../services.ts";
import { param } from "../params.ts";

export function downloadRoutes(services: Services): Router {
  const router = Router();
  const { prisma, config } = services;

  router.get("/:id/versions/:version/package", requireAuth, async (req, res) => {
    const { listing, access } = await visibleListing(
      prisma,
      config,
      param(req, "id"),
      currentUser(res),
    );
    if (!access.admin) {
      if (listing.status === "removed") throw forbidden("this listing was taken down");
      if (!access.mine && !access.owned) throw forbidden("get this listing to download it");
    }
    const version = await prisma.version.findUnique({
      where: { listingId_version: { listingId: listing.id, version: param(req, "version") } },
    });
    if (!version) throw notFound("no such version");
    const range = byteRange(req.query.offset, req.query.limit, version.size);
    if (range) {
      // A part of the package, for clients that cannot take it whole.
      const { offset, length } = range;
      const blob =
        length > 0
          ? await services.blobs.get(version.blobKey, { range: { offset, length } })
          : null;
      if (length > 0 && !blob) throw notFound("no such version");
      const bytes = blob ? Buffer.from(await blob.arrayBuffer()) : Buffer.alloc(0);
      res
        .status(206)
        .set("Content-Range", `bytes ${offset}-${offset + length - 1}/${version.size}`)
        .type("application/octet-stream")
        .send(bytes);
      return;
    }
    const blob = await services.blobs.get(version.blobKey);
    if (!blob) throw notFound("no such version");
    res.type("application/json").send(Buffer.from(await blob.arrayBuffer()));
  });

  return router;
}

/**
 * The bytes `offset` and `limit` ask for of a package of `size`, or
 * null when neither is given.
 */
function byteRange(
  offset: unknown,
  limit: unknown,
  size: number,
): { offset: number; length: number } | null {
  if (offset === undefined && limit === undefined) return null;
  const from = whole(offset ?? "0");
  const most = whole(limit ?? String(size));
  if (from === null || most === null || most < 1) {
    throw badRequest("offset and limit are whole numbers, limit at least 1");
  }
  if (from > size) throw badRequest(`offset is past the package's ${size} bytes`);
  return { offset: from, length: Math.min(most, size - from) };
}

function whole(value: unknown): number | null {
  return typeof value === "string" && /^\d{1,10}$/.test(value) ? Number(value) : null;
}
