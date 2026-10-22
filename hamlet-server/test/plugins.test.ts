import { Buffer } from "node:buffer";
import request from "supertest";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createListing, listedListing } from "./support/flows.ts";
import { type Harness, auth, createHarness } from "./support/harness.ts";
import { agentPackage } from "./support/packages.ts";

// Hamlet's plugins send GET and POST only, set no headers but the body's
// type, and take at most about half a megabyte in one answer.

const MAX_PACKAGE_BYTES = 64 * 1024;

let h: Harness;
let seller: string;
let buyer: string;
beforeAll(async () => {
  h = await createHarness({ maxPackageBytes: MAX_PACKAGE_BYTES });
  seller = await h.signIn("seller");
  buyer = await h.signIn("buyer");
});
afterAll(async () => {
  await h.dispose();
});

function uploadPart(listingId: string, upload: string, index: number, bytes: Buffer) {
  return request(h.app)
    .post(`/v1/listings/${listingId}/uploads/${upload}/parts/${index}`)
    .query({ token: seller })
    .set("Content-Type", "application/octet-stream")
    .send(bytes);
}

async function startUpload(listingId: string): Promise<string> {
  const started = await request(h.app)
    .post(`/v1/listings/${listingId}/uploads`)
    .query({ token: seller })
    .expect(201);
  expect(started.body).toMatchObject({ id: expect.stringMatching(/^up_/), partBytes: 1024 * 1024 });
  return started.body.id as string;
}

/** `bytes` cut into parts of `size`. */
function parts(bytes: Buffer, size: number): Buffer[] {
  const all = [];
  for (let at = 0; at < bytes.length; at += size) all.push(bytes.subarray(at, at + size));
  return all;
}

describe("a session token in the query", () => {
  it("signs the request in", async () => {
    const me = await request(h.app).get("/v1/me").query({ token: buyer }).expect(200);
    expect(me.body.login).toBe("buyer");
  });

  it("leaves a request with an unknown one anonymous", async () => {
    await request(h.app).get("/v1/me").query({ token: "hms_nobody" }).expect(401);
  });

  it("gives way to the header", async () => {
    const me = await request(h.app)
      .get("/v1/me")
      .query({ token: buyer })
      .set(auth(seller))
      .expect(200);
    expect(me.body.login).toBe("seller");
  });
});

describe("a method named in the query", () => {
  it("changes a listing with a POST as PATCH", async () => {
    const listing = await createListing(h, seller);
    const changed = await request(h.app)
      .post(`/v1/listings/${listing.id}`)
      .query({ _method: "PATCH", token: seller })
      .send({ title: "Changed" })
      .expect(200);
    expect(changed.body.title).toBe("Changed");
  });

  it("sets a cover with a POST as PUT", async () => {
    const listing = await createListing(h, seller);
    await request(h.app)
      .post(`/v1/listings/${listing.id}/cover`)
      .query({ _method: "put", token: seller })
      .set("Content-Type", "image/png")
      .send(Buffer.from([0x89, 0x50, 0x4e, 0x47]))
      .expect(204);
  });

  it("signs out with a POST as DELETE", async () => {
    const token = await h.signIn("leaver");
    await request(h.app).post("/v1/auth/session").query({ _method: "DELETE", token }).expect(204);
    await request(h.app).get("/v1/me").query({ token }).expect(401);
  });

  it("is not taken from a GET", async () => {
    const listing = await createListing(h, seller);
    await request(h.app)
      .get(`/v1/listings/${listing.id}`)
      .query({ _method: "DELETE", token: seller })
      .expect(200);
  });
});

describe("a package downloaded in parts", () => {
  it("comes back whole, a part at a time", async () => {
    const listing = await listedListing(h, seller);
    const path = `/v1/listings/${listing.id}/versions/1.0.0/package`;
    const whole = await request(h.app).get(path).query({ token: seller }).expect(200);
    const size = Buffer.from(whole.text).length;
    const got: Buffer[] = [];
    for (let offset = 0; offset < size; offset += 100) {
      const part = await request(h.app)
        .get(path)
        .query({ token: seller, offset, limit: 100 })
        .buffer(true)
        .parse((res, done) => {
          const chunks: Buffer[] = [];
          res.on("data", (chunk: Buffer) => chunks.push(chunk));
          res.on("end", () => done(null, Buffer.concat(chunks)));
        })
        .expect(206);
      expect(part.headers["content-range"]).toBe(
        `bytes ${offset}-${Math.min(offset + 100, size) - 1}/${size}`,
      );
      got.push(part.body as Buffer);
    }
    expect(Buffer.concat(got).toString()).toBe(whole.text);
  });

  it("is empty at the end and refused past it", async () => {
    const listing = await listedListing(h, seller);
    const path = `/v1/listings/${listing.id}/versions/1.0.0/package`;
    const whole = await request(h.app).get(path).query({ token: seller }).expect(200);
    const size = Buffer.from(whole.text).length;
    await request(h.app).get(path).query({ token: seller, offset: size, limit: 10 }).expect(206);
    await request(h.app)
      .get(path)
      .query({ token: seller, offset: size + 1, limit: 10 })
      .expect(400);
    await request(h.app).get(path).query({ token: seller, offset: 0, limit: 0 }).expect(400);
    await request(h.app).get(path).query({ token: seller, offset: "-1" }).expect(400);
  });

  it("is only for those who may download it", async () => {
    const listing = await listedListing(h, seller);
    await request(h.app)
      .get(`/v1/listings/${listing.id}/versions/1.0.0/package`)
      .query({ token: buyer, offset: 0, limit: 10 })
      .expect(403);
  });
});

describe("a package uploaded in parts", () => {
  it("becomes a version as if sent whole, and its parts go", async () => {
    const listing = await createListing(h, seller);
    const bytes = Buffer.from(JSON.stringify(agentPackage(), null, 1));
    const upload = await startUpload(listing.id);
    const all = parts(bytes, 200);
    for (const [index, part] of all.entries()) {
      await uploadPart(listing.id, upload, index, part).expect(204);
    }
    const version = await request(h.app)
      .post(`/v1/listings/${listing.id}/versions`)
      .query({ token: seller })
      .send({ version: "1.0.0", changes: "First.", upload, parts: all.length })
      .expect(201);
    expect(version.body).toMatchObject({ version: "1.0.0", size: bytes.length });
    const back = await request(h.app)
      .get(`/v1/listings/${listing.id}/versions/1.0.0/package`)
      .query({ token: seller })
      .expect(200);
    expect(back.text).toBe(bytes.toString());
    expect(await h.services.blobs.get(`uploads/${listing.id}/${upload}/0`)).toBeNull();
  });

  it("is refused with a part missing", async () => {
    const listing = await createListing(h, seller);
    const upload = await startUpload(listing.id);
    await uploadPart(listing.id, upload, 0, Buffer.from("{")).expect(204);
    const response = await request(h.app)
      .post(`/v1/listings/${listing.id}/versions`)
      .query({ token: seller })
      .send({ version: "1.0.0", upload, parts: 2 })
      .expect(400);
    expect(response.body.error.message).toMatch(/part 1/);
  });

  it("is refused when it is not a package", async () => {
    const listing = await createListing(h, seller);
    const upload = await startUpload(listing.id);
    await uploadPart(listing.id, upload, 0, Buffer.from("not json")).expect(204);
    await request(h.app)
      .post(`/v1/listings/${listing.id}/versions`)
      .query({ token: seller })
      .send({ version: "1.0.0", upload, parts: 1 })
      .expect(400);
  });

  it("is refused over the package limit", async () => {
    const listing = await createListing(h, seller);
    const upload = await startUpload(listing.id);
    const half = Buffer.alloc(MAX_PACKAGE_BYTES / 2 + 1, 0x20);
    await uploadPart(listing.id, upload, 0, half).expect(204);
    await uploadPart(listing.id, upload, 1, half).expect(204);
    const response = await request(h.app)
      .post(`/v1/listings/${listing.id}/versions`)
      .query({ token: seller })
      .send({ version: "1.0.0", upload, parts: 2 })
      .expect(413);
    expect(response.body.error.code).toBe("too_large");
  });

  it("is the seller's alone", async () => {
    const listing = await listedListing(h, seller);
    await request(h.app)
      .post(`/v1/listings/${listing.id}/uploads`)
      .query({ token: buyer })
      .expect(403);
    const upload = await startUpload(listing.id);
    await request(h.app)
      .post(`/v1/listings/${listing.id}/uploads/${upload}/parts/0`)
      .query({ token: buyer })
      .set("Content-Type", "application/octet-stream")
      .send(Buffer.from("{}"))
      .expect(403);
  });

  it("refuses an id or an index it did not make", async () => {
    const listing = await createListing(h, seller);
    await uploadPart(listing.id, "up_../../covers", 0, Buffer.from("x")).expect(404);
    const upload = await startUpload(listing.id);
    await uploadPart(listing.id, upload, 99_999, Buffer.from("x")).expect(400);
    await uploadPart(listing.id, upload, 0, Buffer.alloc(0)).expect(400);
  });
});
