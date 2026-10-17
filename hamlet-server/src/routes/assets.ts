import { Buffer } from "node:buffer";
import { Router } from "express";
import { z } from "zod";
import { ASSET_CATEGORIES, ASSET_TARGETS, assetPreviewKey } from "../assetKeys.ts";
import { ASSET_ORDER, assetJson, ownedAssetIds, ownsAsset } from "../assets.ts";
import { currentUser, maybeUser, requireAuth } from "../auth.ts";
import { conflict, forbidden, notFound } from "../errors.ts";
import type { Asset, User } from "../generated/prisma/client.ts";
import { newId } from "../ids.ts";
import { param } from "../params.ts";
import { settleAssetPurchase, settleFromStripe } from "../settle.ts";
import type { Services } from "../services.ts";

const catalogQuerySchema = z.object({
  target: z.enum(ASSET_TARGETS).optional(),
  category: z.enum(ASSET_CATEGORIES).optional(),
});

const FRAME_PATTERN = /^\d{1,3}$/;

/** The official assets of docs/assets.md, sold by the platform itself. */
export function assetRoutes(services: Services): Router {
  const router = Router();
  const { prisma } = services;

  router.get("/assets", async (req, res) => {
    const query = catalogQuerySchema.parse(req.query);
    const assets = await prisma.asset.findMany({
      where: { status: "listed", ...query },
      orderBy: ASSET_ORDER,
    });
    const owned = await ownedAssetIds(
      prisma,
      maybeUser(res)?.id,
      assets.map((asset) => asset.id),
    );
    res.json({ items: assets.map((asset) => assetJson(asset, owned.has(asset.id))) });
  });

  router.get("/assets/:id", async (req, res) => {
    const asset = await listedAsset(services, param(req, "id"));
    const viewer = maybeUser(res);
    res.json(
      assetJson(asset, viewer !== undefined && (await ownsAsset(prisma, asset.id, viewer.id))),
    );
  });

  // Previews are public, even of an unlisted asset: its owners still see it.
  router.get("/assets/:id/preview/:frame", async (req, res) => {
    const asset = await prisma.asset.findUnique({ where: { id: param(req, "id") } });
    const frameParam = param(req, "frame");
    const frame = FRAME_PATTERN.test(frameParam) ? Number(frameParam) : -1;
    if (!asset || frame < 0 || frame >= asset.frames) throw notFound("no such preview frame");
    const blob = await services.blobs.get(assetPreviewKey(asset.id, asset.version, frame));
    if (!blob) throw notFound("no such preview frame");
    res
      .status(200)
      .type("image/png")
      .set("Cache-Control", "public, max-age=86400")
      .send(Buffer.from(await blob.arrayBuffer()));
  });

  router.post("/assets/:id/purchase", requireAuth, async (req, res) => {
    const buyer = currentUser(res);
    const asset = await listedAsset(services, param(req, "id"));
    if (await ownsAsset(prisma, asset.id, buyer.id)) throw conflict("you already own it");
    if (asset.priceCents === 0) {
      await claimFree(services, asset, buyer);
      res.json({ owned: true });
      return;
    }
    res.json({ owned: false, ...(await startCheckout(services, asset, buyer)) });
  });

  router.get("/asset-purchases/:id", requireAuth, async (req, res) => {
    const id = param(req, "id");
    let purchase = await prisma.assetPurchase.findUnique({ where: { id } });
    if (!purchase || purchase.buyerId !== currentUser(res).id) throw notFound("no such purchase");
    const pending = purchase;
    const settled = await settleFromStripe(services, pending, (outcome) =>
      settleAssetPurchase(services, pending, outcome),
    );
    if (settled) purchase = (await prisma.assetPurchase.findUnique({ where: { id } })) ?? purchase;
    res.json({ id: purchase.id, assetId: purchase.assetId, status: purchase.status });
  });

  // Owners keep an asset that was later unlisted.
  router.get("/assets/:id/file", requireAuth, async (req, res) => {
    const asset = await prisma.asset.findUnique({ where: { id: param(req, "id") } });
    if (!asset) throw notFound("no such asset");
    if (!(await ownsAsset(prisma, asset.id, currentUser(res).id))) {
      throw forbidden("get this asset to download it");
    }
    const blob = await services.blobs.get(asset.fileKey);
    if (!blob) throw notFound("this asset's file is missing");
    // Set directly, so Express does not add a charset to the published type.
    res.setHeader("Content-Type", asset.contentType);
    res
      .status(200)
      .set("Cache-Control", "private, no-store")
      .set("X-Asset-Version", asset.version)
      .send(Buffer.from(await blob.arrayBuffer()));
  });

  return router;
}

async function listedAsset(services: Services, id: string): Promise<Asset> {
  const asset = await services.prisma.asset.findUnique({ where: { id } });
  if (!asset || asset.status !== "listed") throw notFound("no such asset");
  return asset;
}

async function claimFree(services: Services, asset: Asset, buyer: User): Promise<void> {
  const now = services.now();
  await services.prisma.assetPurchase.create({
    data: {
      id: newId("ap"),
      assetId: asset.id,
      buyerId: buyer.id,
      status: "paid",
      amountCents: 0,
      paidAt: now,
      createdAt: now,
      updatedAt: now,
    },
  });
}

async function startCheckout(services: Services, asset: Asset, buyer: User) {
  const now = services.now();
  const purchaseId = newId("ap");
  await services.prisma.assetPurchase.create({
    data: {
      id: purchaseId,
      assetId: asset.id,
      buyerId: buyer.id,
      status: "pending",
      amountCents: asset.priceCents,
      createdAt: now,
      updatedAt: now,
    },
  });
  const session = await services.payments.createPlatformCheckoutSession({
    assetPurchaseId: purchaseId,
    assetId: asset.id,
    buyerId: buyer.id,
    title: asset.title,
    amountCents: asset.priceCents,
    successUrl: `${services.config.publicUrl}/checkout/success`,
    cancelUrl: `${services.config.publicUrl}/checkout/cancel`,
  });
  await services.prisma.assetPurchase.update({
    where: { id: purchaseId },
    data: { stripeSessionId: session.id },
  });
  return { purchaseId, checkoutUrl: session.url };
}
