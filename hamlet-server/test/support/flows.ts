import request from "supertest";
import { auth, type Harness } from "./harness.ts";
import { agentPackage, hamletPackage, pluginPackage } from "./packages.ts";

export interface ListingInput {
  kind?: "agent" | "hamlet" | "plugin";
  title?: string;
  summary?: string;
  category?: string;
  tags?: string[];
  priceCents?: number;
  description?: string;
}

export async function createListing(h: Harness, token: string, input: ListingInput = {}) {
  const response = await request(h.app)
    .post("/v1/listings")
    .set(auth(token))
    .send({
      kind: "agent",
      title: "PR Reviewer",
      summary: "Reviews pull requests.",
      category: "Code",
      priceCents: 0,
      ...input,
    })
    .expect(201);
  return response.body as { id: string; kind: string; status: string };
}

export function packageFor(kind: string) {
  if (kind === "hamlet") return hamletPackage();
  if (kind === "plugin") return pluginPackage();
  return agentPackage();
}

export function uploadVersion(
  h: Harness,
  token: string,
  listingId: string,
  version = "1.0.0",
  kind = "agent",
) {
  return request(h.app)
    .post(`/v1/listings/${listingId}/versions`)
    .set(auth(token))
    .send({ version, changes: "First.", package: packageFor(kind) });
}

/** A listed listing with one version. */
export async function listedListing(h: Harness, token: string, input: ListingInput = {}) {
  const listing = await createListing(h, token, input);
  await uploadVersion(h, token, listing.id, "1.0.0", input.kind ?? "agent").expect(201);
  await request(h.app).post(`/v1/listings/${listing.id}/publish`).set(auth(token)).expect(200);
  return listing;
}
