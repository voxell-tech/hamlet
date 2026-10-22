import { z } from "zod";
import { MAX_PRICE_CENTS, MIN_PAID_CENTS, isValidPrice } from "./money.ts";

export const MAX_TAGS = 8;

export const kindSchema = z.enum(["agent", "hamlet", "plugin"]);

const fields = {
  title: z.string().trim().min(1).max(60),
  summary: z.string().trim().min(1).max(140),
  description: z.string().max(5000),
  category: z.string().trim().min(1).max(32),
  tags: z
    .array(z.string().trim().toLowerCase().min(1).max(24))
    .transform((tags) => [...new Set(tags)])
    .refine((tags) => tags.length <= MAX_TAGS, `at most ${MAX_TAGS} tags`),
  priceCents: z
    .number()
    .int()
    .refine(isValidPrice, `must be 0 or ${MIN_PAID_CENTS} to ${MAX_PRICE_CENTS}`),
};

export const createListingSchema = z.object({
  kind: kindSchema,
  title: fields.title,
  summary: fields.summary,
  description: fields.description.default(""),
  category: fields.category,
  tags: fields.tags.default([]),
  priceCents: fields.priceCents,
});

export const updateListingSchema = z.object({
  title: fields.title.optional(),
  summary: fields.summary.optional(),
  description: fields.description.optional(),
  category: fields.category.optional(),
  tags: fields.tags.optional(),
  priceCents: fields.priceCents.optional(),
});

export const browseQuerySchema = z.object({
  kind: kindSchema.optional(),
  q: z.string().max(200).optional(),
  order: z.enum(["popular", "rated", "cheapest", "newest"]).default("popular"),
  limit: z.coerce.number().int().min(1).max(100).default(50),
  offset: z.coerce.number().int().min(0).default(0),
});

export type BrowseQuery = z.infer<typeof browseQuerySchema>;
