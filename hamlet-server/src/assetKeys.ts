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
/** A preview frame fits in one fetch of a Hamlet plugin and can be shown by it, as a cover. */
export const MAX_PREVIEW_BYTES = 400 * 1024;
export const MAX_PREVIEW_SIDE = 2048;

export type AssetStatus = "listed" | "unlisted";

/** Each published version gets its own keys, so a new one never mixes with the old. */
export function assetFileKey(id: string, version: string): string {
  return `assets/${id}/${version}/file`;
}

export function assetPreviewKey(id: string, version: string, frame: number): string {
  return `assets/${id}/${version}/preview/${frame}.png`;
}
