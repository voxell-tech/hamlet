import { Router } from "express";
import { z } from "zod";
import { currentUser, requireAuth } from "../auth.ts";
import { badRequest, forbidden, notFound } from "../errors.ts";
import { findListing, isAdmin, setStatus } from "../listings.ts";
import type { Services } from "../services.ts";
import { listingJson } from "../views.ts";
import { param } from "../params.ts";

const takedownSchema = z.object({ reason: z.string().trim().min(1).max(1000) });

export function adminRoutes(services: Services): Router {
  const router = Router();
  const { prisma } = services;

  router.use(requireAuth, (_req, res, next) => {
    if (!isAdmin(services.config, currentUser(res))) throw forbidden("admins only");
    next();
  });

  router.post("/listings/:id/takedown", async (req, res) => {
    const { reason } = takedownSchema.parse(req.body);
    const listing = await findListing(prisma, param(req, "id"));
    if (!listing) throw notFound("no such listing");
    res.json(listingJson(await setStatus(prisma, listing.id, "removed", reason)));
  });

  router.post("/listings/:id/restore", async (req, res) => {
    const listing = await findListing(prisma, param(req, "id"));
    if (!listing) throw notFound("no such listing");
    if (listing.status !== "removed") throw badRequest("this listing was not taken down");
    res.json(listingJson(await setStatus(prisma, listing.id, "unlisted")));
  });

  return router;
}
