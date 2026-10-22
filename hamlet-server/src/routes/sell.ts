import { Buffer } from "node:buffer";
import { Router } from "express";
import { z } from "zod";
import { currentUser, requireAuth } from "../auth.ts";
import { COVER_TYPES, MAX_COVER_SIDE, coverBody } from "../body.ts";
import { badRequest, conflict, payoutsRequired, tooLarge } from "../errors.ts";
import { newId, sha256Hex } from "../ids.ts";
import { createListingSchema, updateListingSchema } from "../listingInput.ts";
import { LISTING_INCLUDE, assertNotRemoved, sellersListing, setStatus } from "../listings.ts";
import { readPackageBytes, readUploadedPackage } from "../package/index.ts";
import { refreshPayouts } from "../payouts.ts";
import type { Services } from "../services.ts";
import { compareVersions, parseVersion } from "../versionNumber.ts";
import { type ListingRow, listingJson, versionJson } from "../views.ts";
import { param } from "../params.ts";
import { pictureSize } from "../picture.ts";
import { assembleUpload, dropUpload } from "./uploads.ts";

const uploadSchema = z.object({
  version: z.string().max(40),
  changes: z.string().max(2000).default(""),
  /** A package sent in parts beforehand, in place of `package`. */
  upload: z.string().max(64).optional(),
  parts: z.number().int().optional(),
});

export function sellRoutes(services: Services): Router {
  const router = Router();
  const { prisma, config } = services;

  router.post("/", requireAuth, async (req, res) => {
    const input = createListingSchema.parse(req.body);
    const now = services.now();
    const listing = await prisma.listing.create({
      data: {
        id: newId("l"),
        ...input,
        tags: JSON.stringify(input.tags),
        status: "draft",
        sellerId: currentUser(res).id,
        createdAt: now,
        updatedAt: now,
      },
      include: LISTING_INCLUDE,
    });
    res.status(201).json(listingJson(listing));
  });

  router.patch("/:id", requireAuth, async (req, res) => {
    const listing = await sellersListing(prisma, config, param(req, "id"), currentUser(res));
    assertNotRemoved(listing);
    const input = updateListingSchema.parse(req.body);
    if (input.priceCents && listing.status === "listed") {
      await requirePayouts(services, listing);
    }
    const { tags, ...rest } = input;
    const changed = Object.values(input).some((value) => value !== undefined);
    const updated = await prisma.listing.update({
      where: { id: listing.id },
      data: {
        ...rest,
        ...(tags ? { tags: JSON.stringify(tags) } : {}),
        ...(changed ? { updatedAt: services.now() } : {}),
      },
      include: LISTING_INCLUDE,
    });
    res.json(listingJson(updated));
  });

  router.put("/:id/cover", requireAuth, coverBody, async (req, res) => {
    const listing = await sellersListing(prisma, config, param(req, "id"), currentUser(res));
    assertNotRemoved(listing);
    const contentType = req.is(COVER_TYPES);
    if (!contentType || !Buffer.isBuffer(req.body) || req.body.length === 0) {
      throw badRequest(`send the image bytes with Content-Type ${COVER_TYPES.join(", ")}`);
    }
    const size = pictureSize(req.body);
    if (size?.type !== contentType) {
      throw badRequest(`the cover is not a ${contentType} whose size can be read`);
    }
    if (Math.max(size.width, size.height) > MAX_COVER_SIDE) {
      throw tooLarge(
        `a cover is at most ${MAX_COVER_SIDE} pixels a side, this one is ${size.width} by ${size.height}`,
      );
    }
    const coverKey = `covers/${listing.id}`;
    await services.blobs.put(coverKey, req.body, { httpMetadata: { contentType } });
    await prisma.listing.update({
      where: { id: listing.id },
      data: { coverKey, coverType: contentType, coverVersion: { increment: 1 } },
    });
    res.status(204).end();
  });

  router.delete("/:id/cover", requireAuth, async (req, res) => {
    const listing = await sellersListing(prisma, config, param(req, "id"), currentUser(res));
    if (listing.coverKey) {
      await services.blobs.delete(listing.coverKey);
      await prisma.listing.update({
        where: { id: listing.id },
        data: { coverKey: null, coverType: null },
      });
    }
    res.status(204).end();
  });

  router.post("/:id/versions", requireAuth, async (req, res) => {
    const listing = await sellersListing(prisma, config, param(req, "id"), currentUser(res));
    assertNotRemoved(listing);
    const input = uploadSchema.parse(req.body);
    const number = parseVersion(input.version);
    if (!number) throw badRequest("version must be MAJOR.MINOR.PATCH, digits only");
    const body = req.body as { package?: unknown };
    const pkg =
      input.upload === undefined
        ? readUploadedPackage(req.rawBody, body.package, listing.kind, config.maxPackageBytes)
        : readPackageBytes(
            await assembleUpload(services, listing.id, input.upload, input.parts ?? 1),
            listing.kind,
            config.maxPackageBytes,
          );
    const latest = listing.versions[0];
    if (latest && compareVersions(number, latest) <= 0) {
      throw conflict(`version must be greater than ${latest.version}`);
    }
    const blobKey = `packages/${listing.id}/${input.version}.json`;
    await services.blobs.put(blobKey, pkg.bytes, {
      httpMetadata: { contentType: "application/json" },
    });
    const now = services.now();
    const version = await prisma.version.create({
      data: {
        id: newId("v"),
        listingId: listing.id,
        version: input.version,
        ...number,
        changes: input.changes,
        blobKey,
        size: pkg.bytes.byteLength,
        sha256: sha256Hex(pkg.bytes),
        meta: JSON.stringify(pkg.meta),
        createdAt: now,
      },
    });
    await prisma.listing.update({ where: { id: listing.id }, data: { updatedAt: now } });
    if (input.upload !== undefined) {
      await dropUpload(services, listing.id, input.upload, input.parts ?? 1);
    }
    res.status(201).json(versionJson(version));
  });

  router.post("/:id/publish", requireAuth, async (req, res) => {
    const listing = await sellersListing(prisma, config, param(req, "id"), currentUser(res));
    assertNotRemoved(listing);
    if (listing.versions.length === 0) throw badRequest("upload a version before publishing");
    if (listing.priceCents > 0) await requirePayouts(services, listing);
    res.json(listingJson(await setStatus(prisma, listing.id, "listed")));
  });

  router.post("/:id/unlist", requireAuth, async (req, res) => {
    const listing = await sellersListing(prisma, config, param(req, "id"), currentUser(res));
    assertNotRemoved(listing);
    if (listing.status === "draft") throw badRequest("a draft is not published yet");
    res.json(listingJson(await setStatus(prisma, listing.id, "unlisted")));
  });

  return router;
}

async function requirePayouts(services: Services, listing: ListingRow): Promise<void> {
  const seller = await refreshPayouts(services, listing.seller);
  if (!seller.payoutsEnabled) {
    throw payoutsRequired("set up payouts before selling for a price");
  }
}
