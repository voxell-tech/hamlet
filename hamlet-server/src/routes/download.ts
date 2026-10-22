import { Buffer } from "node:buffer";
import { Router } from "express";
import { currentUser, requireAuth } from "../auth.ts";
import { byteRange, sendRange } from "../byteRange.ts";
import { forbidden, notFound } from "../errors.ts";
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
      if (!(await sendRange(res, services.blobs, version.blobKey, range, version.size))) {
        throw notFound("no such version");
      }
      return;
    }
    const blob = await services.blobs.get(version.blobKey);
    if (!blob) throw notFound("no such version");
    res.type("application/json").send(Buffer.from(await blob.arrayBuffer()));
  });

  return router;
}
