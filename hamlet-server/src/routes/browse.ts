import { Buffer } from "node:buffer";
import { Router } from "express";
import type { Prisma } from "../generated/prisma/client.ts";
import { maybeUser } from "../auth.ts";
import { notFound } from "../errors.ts";
import { type BrowseQuery, browseQuerySchema } from "../listingInput.ts";
import { LISTING_INCLUDE, visibleListing } from "../listings.ts";
import type { Services } from "../services.ts";
import { NEWEST_VERSION_FIRST } from "../versionNumber.ts";
import { listingJson, reviewJson, versionJson } from "../views.ts";
import { param } from "../params.ts";

const MAX_REVIEWS = 50;
const MAX_QUERY_WORDS = 10;

export function browseRoutes(services: Services): Router {
  const router = Router();
  const { prisma, config } = services;

  router.get("/", async (req, res) => {
    const query = browseQuerySchema.parse(req.query);
    const where = browseWhere(query);
    const [listings, total] = await Promise.all([
      prisma.listing.findMany({
        where,
        orderBy: browseOrder(query.order),
        skip: query.offset,
        take: query.limit,
        include: LISTING_INCLUDE,
      }),
      prisma.listing.count({ where }),
    ]);
    res.json({ items: listings.map(listingJson), total });
  });

  router.get("/:id", async (req, res) => {
    const { listing, access } = await visibleListing(
      prisma,
      config,
      param(req, "id"),
      maybeUser(res),
    );
    const [versions, reviews] = await Promise.all([
      prisma.version.findMany({
        where: { listingId: listing.id },
        orderBy: NEWEST_VERSION_FIRST,
      }),
      prisma.review.findMany({
        where: { listingId: listing.id },
        orderBy: [{ createdAt: "desc" }, { id: "asc" }],
        take: MAX_REVIEWS,
        include: { user: true },
      }),
    ]);
    res.json({
      listing: listingJson(listing),
      versions: versions.map(versionJson),
      reviews: reviews.map(reviewJson),
      owned: access.owned,
      mine: access.mine,
    });
  });

  router.get("/:id/cover", async (req, res) => {
    const { listing } = await visibleListing(prisma, config, param(req, "id"), maybeUser(res));
    const blob = listing.coverKey ? await services.blobs.get(listing.coverKey) : null;
    if (!blob) throw notFound("this listing has no cover");
    res
      .status(200)
      .type(listing.coverType ?? "application/octet-stream")
      .set("Cache-Control", "private, max-age=300")
      .send(Buffer.from(await blob.arrayBuffer()));
  });

  return router;
}

function browseWhere(query: BrowseQuery): Prisma.ListingWhereInput {
  const words = (query.q ?? "").split(/\s+/).filter(Boolean).slice(0, MAX_QUERY_WORDS);
  // SQLite LIKE, which `contains` uses, ignores ASCII case.
  return {
    status: "listed",
    versions: { some: {} },
    ...(query.kind ? { kind: query.kind } : {}),
    AND: words.map((word) => ({
      OR: [
        { title: { contains: word } },
        { summary: { contains: word } },
        { category: { contains: word } },
        { tags: { contains: word } },
        { seller: { login: { contains: word } } },
      ],
    })),
  };
}

function browseOrder(order: BrowseQuery["order"]): Prisma.ListingOrderByWithRelationInput[] {
  const byTitle: Prisma.ListingOrderByWithRelationInput[] = [{ title: "asc" }, { id: "asc" }];
  switch (order) {
    case "popular":
      return [{ ownersCount: "desc" }, ...byTitle];
    case "rated":
      return [{ starsAvg: "desc" }, { reviewsCount: "desc" }, ...byTitle];
    case "cheapest":
      return [{ priceCents: "asc" }, ...byTitle];
    case "newest":
      return [{ updatedAt: "desc" }, ...byTitle];
  }
}
