import type { Asset, Prisma, PrismaClient } from "./generated/prisma/client.ts";
import { CURRENCY } from "./money.ts";

/** The catalog's order: target, then category, then price, then title. */
export const ASSET_ORDER: Prisma.AssetOrderByWithRelationInput[] = [
  { target: "asc" },
  { category: "asc" },
  { priceCents: "asc" },
  { title: "asc" },
  { id: "asc" },
];

export function assetJson(asset: Asset, owned: boolean) {
  const id = encodeURIComponent(asset.id);
  return {
    id: asset.id,
    target: asset.target,
    category: asset.category,
    title: asset.title,
    summary: asset.summary,
    priceCents: asset.priceCents,
    currency: CURRENCY,
    version: asset.version,
    frames: asset.frames,
    previewUrl: `/v1/assets/${id}/preview/{frame}?v=${encodeURIComponent(asset.version)}`,
    owned,
    updatedAt: asset.updatedAt.toISOString(),
  };
}

/** The ids of the assets `userId` owns, among `assetIds` when given. */
export async function ownedAssetIds(
  prisma: PrismaClient,
  userId: string | undefined,
  assetIds?: string[],
): Promise<Set<string>> {
  if (!userId) return new Set();
  const purchases = await prisma.assetPurchase.findMany({
    where: {
      buyerId: userId,
      status: "paid",
      ...(assetIds ? { assetId: { in: assetIds } } : {}),
    },
    distinct: ["assetId"],
    select: { assetId: true },
  });
  return new Set(purchases.map((purchase) => purchase.assetId));
}

export async function ownsAsset(
  prisma: PrismaClient,
  assetId: string,
  userId: string,
): Promise<boolean> {
  return (await ownedAssetIds(prisma, userId, [assetId])).has(assetId);
}
