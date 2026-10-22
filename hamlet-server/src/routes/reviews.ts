import { Router } from "express";
import { z } from "zod";
import { currentUser, requireAuth } from "../auth.ts";
import { forbidden } from "../errors.ts";
import { newId } from "../ids.ts";
import { refreshListingStats, visibleListing } from "../listings.ts";
import type { Services } from "../services.ts";
import { reviewJson } from "../views.ts";
import { param } from "../params.ts";

const reviewSchema = z.object({
  stars: z.number().int().min(1).max(5),
  words: z.string().max(1000).default(""),
});

export function reviewRoutes(services: Services): Router {
  const router = Router();
  const { prisma, config } = services;

  router.post("/:id/reviews", requireAuth, async (req, res) => {
    const user = currentUser(res);
    const { listing, access } = await visibleListing(prisma, config, param(req, "id"), user);
    if (access.mine) throw forbidden("you cannot review your own listing");
    if (!access.owned) throw forbidden("only owners can review");
    const input = reviewSchema.parse(req.body);
    const now = services.now();
    // Posting again replaces the review, so it also takes the new time.
    const fields = { stars: input.stars, words: input.words, createdAt: now, updatedAt: now };
    const review = await prisma.review.upsert({
      where: { listingId_userId: { listingId: listing.id, userId: user.id } },
      update: fields,
      create: { id: newId("r"), listingId: listing.id, userId: user.id, ...fields },
      include: { user: true },
    });
    await refreshListingStats(prisma, listing.id);
    res.json(reviewJson(review));
  });

  return router;
}
