import type { Prisma, PrismaClient, User } from "./generated/prisma/client.ts";
import { forbidden, notFound } from "./errors.ts";
import type { Config } from "./services.ts";
import { NEWEST_VERSION_FIRST } from "./versionNumber.ts";
import type { ListingRow } from "./views.ts";

export type ListingStatus = "draft" | "listed" | "unlisted" | "removed";

/** Loads what `listingJson` needs: the seller and the latest version. */
export const LISTING_INCLUDE = {
  seller: true,
  versions: { orderBy: NEWEST_VERSION_FIRST, take: 1 },
} satisfies Prisma.ListingInclude;

export interface Access {
  visible: boolean;
  owned: boolean;
  mine: boolean;
  admin: boolean;
}

export function isAdmin(config: Config, user: User | undefined): boolean {
  return user !== undefined && config.adminLogins.includes(user.login.toLowerCase());
}

export async function findListing(prisma: PrismaClient, id: string): Promise<ListingRow | null> {
  return prisma.listing.findUnique({ where: { id }, include: LISTING_INCLUDE });
}

export async function owns(
  prisma: PrismaClient,
  listingId: string,
  userId: string,
): Promise<boolean> {
  const purchase = await prisma.purchase.findFirst({
    where: { listingId, buyerId: userId, status: "paid" },
    select: { id: true },
  });
  return purchase !== null;
}

/**
 * Listed is public. Otherwise the seller and admins see it, and owners do
 * unless it was taken down.
 */
export async function accessFor(
  prisma: PrismaClient,
  config: Config,
  listing: ListingRow,
  viewer: User | undefined,
): Promise<Access> {
  const mine = viewer !== undefined && listing.sellerId === viewer.id;
  const admin = isAdmin(config, viewer);
  const owned = viewer !== undefined && (await owns(prisma, listing.id, viewer.id));
  const status = listing.status as ListingStatus;
  const visible = status === "listed" || mine || admin || (status !== "removed" && owned);
  return { visible, owned, mine, admin };
}

/** The listing when the viewer may see it, else 404. */
export async function visibleListing(
  prisma: PrismaClient,
  config: Config,
  id: string,
  viewer: User | undefined,
): Promise<{ listing: ListingRow; access: Access }> {
  const listing = await findListing(prisma, id);
  if (!listing) throw notFound("no such listing");
  const access = await accessFor(prisma, config, listing, viewer);
  if (!access.visible) throw notFound("no such listing");
  return { listing, access };
}

/** The seller's own listing; 404 when invisible, 403 for anyone else. */
export async function sellersListing(
  prisma: PrismaClient,
  config: Config,
  id: string,
  seller: User,
): Promise<ListingRow> {
  const { listing, access } = await visibleListing(prisma, config, id, seller);
  if (!access.mine) throw forbidden("only the seller can do that");
  return listing;
}

export function assertNotRemoved(listing: ListingRow): void {
  if (listing.status === "removed") throw forbidden("this listing was taken down");
}

/** Recounts owners and reviews. Safe to call any number of times. */
export async function refreshListingStats(prisma: PrismaClient, listingId: string): Promise<void> {
  const owners = await prisma.purchase.findMany({
    where: { listingId, status: "paid" },
    distinct: ["buyerId"],
    select: { buyerId: true },
  });
  const reviews = await prisma.review.aggregate({
    where: { listingId },
    _count: { _all: true },
    _avg: { stars: true },
  });
  await prisma.listing.update({
    where: { id: listingId },
    data: {
      ownersCount: owners.length,
      reviewsCount: reviews._count._all,
      starsAvg: reviews._avg.stars ?? 0,
    },
  });
}

export async function setStatus(
  prisma: PrismaClient,
  listingId: string,
  status: Exclude<ListingStatus, "draft">,
  removedReason: string | null = null,
): Promise<ListingRow> {
  return prisma.listing.update({
    where: { id: listingId },
    data: { status, removedReason },
    include: LISTING_INCLUDE,
  });
}
