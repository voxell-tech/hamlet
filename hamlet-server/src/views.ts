import type { Listing, Review, User, Version } from "./generated/prisma/client.ts";
import { CURRENCY } from "./money.ts";

/** A listing loaded with `LISTING_INCLUDE`. */
export type ListingRow = Listing & { seller: User; versions: Version[] };

export function userJson(user: User) {
  return {
    id: user.id,
    login: user.login,
    name: user.name,
    avatarUrl: user.avatarUrl,
  };
}

export function meJson(user: User) {
  return {
    ...userJson(user),
    payouts: { connected: user.stripeAccountId !== null, enabled: user.payoutsEnabled },
  };
}

export function sellerJson(user: User) {
  return { ...userJson(user), verified: user.payoutsEnabled };
}

export function versionJson(version: Version) {
  return {
    version: version.version,
    changes: version.changes,
    size: version.size,
    sha256: version.sha256,
    createdAt: version.createdAt.toISOString(),
    meta: JSON.parse(version.meta) as unknown,
  };
}

export function listingJson(row: ListingRow) {
  const latest = row.versions[0];
  return {
    id: row.id,
    kind: row.kind,
    title: row.title,
    summary: row.summary,
    description: row.description,
    category: row.category,
    tags: JSON.parse(row.tags) as string[],
    priceCents: row.priceCents,
    currency: CURRENCY,
    status: row.status,
    seller: sellerJson(row.seller),
    coverUrl: row.coverKey ? `/v1/listings/${row.id}/cover?v=${row.coverVersion}` : null,
    latest: latest ? versionJson(latest) : null,
    owners: row.ownersCount,
    stars: Math.round(row.starsAvg * 100) / 100,
    reviews: row.reviewsCount,
    createdAt: row.createdAt.toISOString(),
    updatedAt: row.updatedAt.toISOString(),
  };
}

export function reviewJson(review: Review & { user: User }) {
  return {
    by: { login: review.user.login, avatarUrl: review.user.avatarUrl },
    stars: review.stars,
    words: review.words,
    createdAt: review.createdAt.toISOString(),
  };
}
