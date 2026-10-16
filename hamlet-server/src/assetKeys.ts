/**
 * What the server and `pnpm assets:publish` agree on about official
 * assets: their kinds and where their bytes live in R2. Kept free of
 * Prisma and Express so the publish script can import it.
 */

export const ASSET_TARGETS = ["agent", "hamlet"] as const;
export const ASSET_CATEGORIES = ["emote", "skin", "mesh"] as const;
export type AssetTarget = (typeof ASSET_TARGETS)[number];
export type AssetCategory = (typeof ASSET_CATEGORIES)[number];

export const ASSET_ID_PATTERN = /^[a-z0-9-]{1,64}$/;
export const MAX_FRAMES = 72;

export type AssetStatus = "listed" | "unlisted";

/** Each published version gets its own keys, so a new one never mixes with the old. */
export function assetFileKey(id: string, version: string): string {
  return `assets/${id}/${version}/file`;
}

export function assetPreviewKey(id: string, version: string, frame: number): string {
  return `assets/${id}/${version}/preview/${frame}.png`;
}
