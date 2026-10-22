import { Buffer } from "node:buffer";
import request from "supertest";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { sha256Hex } from "../src/ids.ts";
import { createListing, uploadVersion } from "./support/flows.ts";
import { type Harness, auth, createHarness } from "./support/harness.ts";
import { jpeg, png, webp } from "./support/pictures.ts";
import {
  agent,
  agentPackage,
  file,
  hamletPackage,
  manifest,
  pluginPackage,
} from "./support/packages.ts";

const MAX_PACKAGE_BYTES = 64 * 1024;

let h: Harness;
let seller: string;
let stranger: string;
beforeAll(async () => {
  h = await createHarness({ maxPackageBytes: MAX_PACKAGE_BYTES });
  seller = await h.signIn("seller");
  stranger = await h.signIn("stranger");
});
afterAll(async () => {
  await h.dispose();
});

function upload(listingId: string, body: unknown, token = seller) {
  return request(h.app)
    .post(`/v1/listings/${listingId}/versions`)
    .set(auth(token))
    .send(body as object);
}

describe("creating a listing", () => {
  it("makes a draft with trimmed text and clean tags", async () => {
    const response = await request(h.app)
      .post("/v1/listings")
      .set(auth(seller))
      .send({
        kind: "agent",
        title: "  PR Reviewer  ",
        summary: "Reviews pull requests.",
        category: "Code",
        tags: ["GitHub", "review", "github"],
        priceCents: 900,
      })
      .expect(201);
    expect(response.body).toMatchObject({
      id: expect.stringMatching(/^l_/),
      kind: "agent",
      title: "PR Reviewer",
      description: "",
      tags: ["github", "review"],
      priceCents: 900,
      currency: "usd",
      status: "draft",
      seller: { login: "seller", verified: false },
      coverUrl: null,
      latest: null,
      owners: 0,
      stars: 0,
      reviews: 0,
    });
  });

  it("needs a session", async () => {
    await request(h.app).post("/v1/listings").send({}).expect(401);
  });

  it.each([
    ["a bad price", { priceCents: 49 }],
    ["a price too high", { priceCents: 50_001 }],
    ["a fractional price", { priceCents: 100.5 }],
    ["a title too long", { title: "x".repeat(61) }],
    ["a blank title", { title: "   " }],
    ["a summary too long", { summary: "x".repeat(141) }],
    ["too many tags", { tags: ["a", "b", "c", "d", "e", "f", "g", "h", "i"] }],
    ["a tag too long", { tags: ["x".repeat(25)] }],
    ["an unknown kind", { kind: "robot" }],
  ])("rejects %s", async (_name, change) => {
    const response = await request(h.app)
      .post("/v1/listings")
      .set(auth(seller))
      .send({ kind: "agent", title: "T", summary: "S", category: "C", priceCents: 0, ...change })
      .expect(400);
    expect(response.body.error.code).toBe("bad_request");
  });
});

describe("editing a listing", () => {
  it("lets the seller change text and moves updatedAt", async () => {
    const listing = await createListing(h, seller);
    const before = await h.prisma.listing.findUniqueOrThrow({ where: { id: listing.id } });
    const response = await request(h.app)
      .patch(`/v1/listings/${listing.id}`)
      .set(auth(seller))
      .send({ title: "Better", tags: ["New"] })
      .expect(200);
    expect(response.body.title).toBe("Better");
    expect(response.body.tags).toEqual(["new"]);
    expect(Date.parse(response.body.updatedAt)).toBeGreaterThan(before.updatedAt.getTime());
  });

  it("is only for the seller", async () => {
    const listing = await createListing(h, seller);
    // A draft is invisible to others, so it is not found rather than forbidden.
    await request(h.app)
      .patch(`/v1/listings/${listing.id}`)
      .set(auth(stranger))
      .send({})
      .expect(404);
  });

  it("needs payouts to make a listed listing paid", async () => {
    const listing = await createListing(h, seller);
    await uploadVersion(h, seller, listing.id).expect(201);
    await request(h.app).post(`/v1/listings/${listing.id}/publish`).set(auth(seller)).expect(200);
    const response = await request(h.app)
      .patch(`/v1/listings/${listing.id}`)
      .set(auth(seller))
      .send({ priceCents: 500 })
      .expect(403);
    expect(response.body.error.code).toBe("payouts_required");
  });
});

describe("uploading versions", () => {
  it("stores an agent package as sent and derives its meta", async () => {
    const listing = await createListing(h, seller);
    const raw = `{"version":"1.0.0","changes":"First.","package":  ${JSON.stringify(agentPackage(), null, 1)} }`;
    const response = await request(h.app)
      .post(`/v1/listings/${listing.id}/versions`)
      .set(auth(seller))
      .set("Content-Type", "application/json")
      .send(raw)
      .expect(201);
    const sent = Buffer.from(JSON.stringify(agentPackage(), null, 1));
    expect(response.body).toEqual({
      version: "1.0.0",
      changes: "First.",
      size: sent.length,
      sha256: sha256Hex(sent),
      createdAt: expect.any(String),
      meta: {
        name: "pr-reviewer",
        purpose: "Reviews pull requests.",
        model: "anthropic/claude-sonnet-4",
        character: "# Reviewer\nCareful and kind.",
        skills: ["review", "tests"],
        notes: true,
      },
    });
    const download = await request(h.app)
      .get(`/v1/listings/${listing.id}/versions/1.0.0/package`)
      .set(auth(seller))
      .buffer(true)
      .parse((res, done) => {
        const chunks: Buffer[] = [];
        res.on("data", (chunk: Buffer) => chunks.push(chunk));
        res.on("end", () => done(null, Buffer.concat(chunks)));
      })
      .expect(200);
    expect(Buffer.compare(download.body as Buffer, sent)).toBe(0);
  });

  it("cuts an agent's character to 4000 chars and allows a missing model", async () => {
    const listing = await createListing(h, seller);
    const pkg = agentPackage({ model: undefined, files: [file("CHARACTER.md", "é".repeat(5000))] });
    const response = await upload(listing.id, { version: "1.0.0", package: pkg }).expect(201);
    expect(response.body.meta.character).toBe("é".repeat(4000));
    expect(response.body.meta).toMatchObject({ model: null, skills: [], notes: false });
    expect(response.body.changes).toBe("");
  });

  it("derives hamlet meta", async () => {
    const listing = await createListing(h, seller, { kind: "hamlet" });
    const response = await upload(listing.id, {
      version: "0.1.0",
      package: hamletPackage(),
    }).expect(201);
    expect(response.body.meta).toEqual({
      name: "launch-crew",
      title: "Launch Crew",
      kind: "SaaS launch team",
      purpose: "Ships a small product.",
      seed: "18446744073709551615",
      mayor: {
        name: "mayor",
        purpose: "Reviews pull requests.",
        model: null,
        character: "# Reviewer\nCareful and kind.",
        skills: ["review", "tests"],
        notes: true,
      },
      agents: [
        expect.objectContaining({ name: "builder", model: "anthropic/claude-sonnet-4" }),
        {
          name: "writer",
          purpose: "Writes docs.",
          model: null,
          character: "",
          skills: [],
          notes: false,
        },
      ],
    });
  });

  it("derives plugin meta with permissions from each manifest", async () => {
    const listing = await createListing(h, seller, { kind: "plugin" });
    const response = await upload(listing.id, {
      version: "1.0.0",
      package: pluginPackage(),
    }).expect(201);
    expect(response.body.meta).toEqual({
      tools: [
        {
          name: "calendar",
          description: "Reads your calendar.",
          permissions: ["run_program", "network", "write_files"],
        },
        { name: "clock", description: "", permissions: ["run_program", "read_files"] },
        { name: "plain", description: "", permissions: ["run_program"] },
      ],
    });
  });

  const invalid: [string, () => unknown, RegExp][] = [
    [
      "path traversal",
      () => agentPackage({ files: [file("workspace/skills/a/../../x", "x")] }),
      /\.\./,
    ],
    ["a parent path", () => agentPackage({ files: [file("../x", "x")] }), /\.\./],
    ["an absolute path", () => agentPackage({ files: [file("/etc/passwd", "x")] }), /relative/],
    ["a backslash", () => agentPackage({ files: [file("workspace\\NOTES.md", "x")] }), /\\/],
    [
      "a file outside the house",
      () => agentPackage({ files: [file("secrets.txt", "x")] }),
      /not allowed/,
    ],
    [
      "bad base64",
      () => agentPackage({ files: [{ path: "CHARACTER.md", data: "not base64!" }] }),
      /base64/,
    ],
    [
      "a duplicate path",
      () => agentPackage({ files: [file("CHARACTER.md", "a"), file("CHARACTER.md", "b")] }),
      /twice/,
    ],
    ["a bad agent name", () => agentPackage({ name: "Bad Name" }), /name/],
    ["the wrong kind", () => pluginPackage(), /kind/],
    ["the wrong format", () => ({ ...agentPackage(), format: 2 }), /format/],
  ];

  it.each(invalid)("rejects %s", async (_name, build, message) => {
    const listing = await createListing(h, seller);
    const response = await upload(listing.id, { version: "1.0.0", package: build() }).expect(400);
    expect(response.body.error.code).toBe("bad_request");
    expect(response.body.error.message).toMatch(message);
  });

  it("rejects a plugin tool without manifest.json, or with a bad one", async () => {
    const listing = await createListing(h, seller, { kind: "plugin" });
    const noManifest = {
      format: 1,
      kind: "plugin",
      plugin: { tools: [{ name: "x", files: [file("run", "x")] }] },
    };
    const missing = await upload(listing.id, { version: "1.0.0", package: noManifest }).expect(400);
    expect(missing.body.error.message).toMatch(/manifest\.json/);
    const wrongName = {
      format: 1,
      kind: "plugin",
      plugin: { tools: [{ name: "x", files: [manifest("y")] }] },
    };
    await upload(listing.id, { version: "1.0.0", package: wrongName }).expect(400);
    const noExec = {
      format: 1,
      kind: "plugin",
      plugin: { tools: [{ name: "x", files: [file("manifest.json", '{"name":"x","exec":[]}')] }] },
    };
    await upload(listing.id, { version: "1.0.0", package: noExec }).expect(400);
    const noTools = { format: 1, kind: "plugin", plugin: { tools: [] } };
    await upload(listing.id, { version: "1.0.0", package: noTools }).expect(400);
  });

  it("rejects a hamlet with a repeated agent name or a seed beyond u64", async () => {
    const listing = await createListing(h, seller, { kind: "hamlet" });
    const repeated = hamletPackage();
    repeated.hamlet.agents.push(agent("mayor"));
    await upload(listing.id, { version: "1.0.0", package: repeated }).expect(400);
    const bigSeed = hamletPackage();
    bigSeed.hamlet.seed = "18446744073709551616";
    await upload(listing.id, { version: "1.0.0", package: bigSeed }).expect(400);
  });

  it("answers 413 for a package over the limit", async () => {
    const listing = await createListing(h, seller);
    const big = agentPackage({ files: [file("CHARACTER.md", "x".repeat(MAX_PACKAGE_BYTES))] });
    const response = await upload(listing.id, { version: "1.0.0", package: big }).expect(413);
    expect(response.body.error.code).toBe("too_large");
    const huge = agentPackage({ files: [file("CHARACTER.md", "x".repeat(MAX_PACKAGE_BYTES * 2))] });
    const hugeResponse = await upload(listing.id, { version: "1.0.0", package: huge }).expect(413);
    expect(hugeResponse.body.error.code).toBe("too_large");
  });

  it("needs each version to be greater than the last", async () => {
    const listing = await createListing(h, seller);
    await upload(listing.id, { version: "1.2.0", package: agentPackage() }).expect(201);
    for (const version of ["1.2.0", "1.1.9", "0.9.0"]) {
      const response = await upload(listing.id, { version, package: agentPackage() }).expect(409);
      expect(response.body.error.code).toBe("conflict");
    }
    await upload(listing.id, { version: "1.10.0", package: agentPackage() }).expect(201);
    const detail = await request(h.app)
      .get(`/v1/listings/${listing.id}`)
      .set(auth(seller))
      .expect(200);
    expect(detail.body.versions.map((v: { version: string }) => v.version)).toEqual([
      "1.10.0",
      "1.2.0",
    ]);
    expect(detail.body.listing.latest.version).toBe("1.10.0");
  });

  it("rejects a malformed version and uploads by others", async () => {
    const listing = await createListing(h, seller);
    await upload(listing.id, { version: "1.0", package: agentPackage() }).expect(400);
    await upload(listing.id, { version: "1.0.0" }).expect(400);
    await upload(listing.id, { version: "1.0.0", package: agentPackage() }, stranger).expect(404);
  });
});

describe("publishing", () => {
  it("needs a version first", async () => {
    const listing = await createListing(h, seller);
    const response = await request(h.app)
      .post(`/v1/listings/${listing.id}/publish`)
      .set(auth(seller))
      .expect(400);
    expect(response.body.error.code).toBe("bad_request");
  });

  it("needs payouts for a paid listing", async () => {
    const listing = await createListing(h, seller, { priceCents: 900 });
    await uploadVersion(h, seller, listing.id).expect(201);
    const response = await request(h.app)
      .post(`/v1/listings/${listing.id}/publish`)
      .set(auth(seller))
      .expect(403);
    expect(response.body.error.code).toBe("payouts_required");

    const paidSeller = await h.signInSeller("paid-seller");
    const paid = await createListing(h, paidSeller, { priceCents: 900 });
    await uploadVersion(h, paidSeller, paid.id).expect(201);
    const listed = await request(h.app)
      .post(`/v1/listings/${paid.id}/publish`)
      .set(auth(paidSeller))
      .expect(200);
    expect(listed.body).toMatchObject({ status: "listed", seller: { verified: true } });
  });

  it("lists, unlists and lists again", async () => {
    const listing = await createListing(h, seller);
    await request(h.app).post(`/v1/listings/${listing.id}/unlist`).set(auth(seller)).expect(400);
    await uploadVersion(h, seller, listing.id).expect(201);
    const listed = await request(h.app)
      .post(`/v1/listings/${listing.id}/publish`)
      .set(auth(seller))
      .expect(200);
    expect(listed.body.status).toBe("listed");
    const unlisted = await request(h.app)
      .post(`/v1/listings/${listing.id}/unlist`)
      .set(auth(seller))
      .expect(200);
    expect(unlisted.body.status).toBe("unlisted");
    await request(h.app).post(`/v1/listings/${listing.id}/publish`).set(auth(seller)).expect(200);
    await request(h.app).post(`/v1/listings/${listing.id}/unlist`).set(auth(stranger)).expect(403);
  });
});

describe("covers", () => {
  const cover = png(640, 360);

  it("uploads, serves, replaces and deletes a cover", async () => {
    const listing = await createListing(h, seller);
    await request(h.app)
      .put(`/v1/listings/${listing.id}/cover`)
      .set(auth(seller))
      .set("Content-Type", "image/png")
      .send(cover)
      .expect(204);
    const detail = await request(h.app)
      .get(`/v1/listings/${listing.id}`)
      .set(auth(seller))
      .expect(200);
    expect(detail.body.listing.coverUrl).toBe(`/v1/listings/${listing.id}/cover?v=1`);

    const served = await request(h.app)
      .get(`/v1/listings/${listing.id}/cover`)
      .set(auth(seller))
      .expect(200);
    expect(served.headers["content-type"]).toBe("image/png");
    expect(Buffer.compare(served.body as Buffer, cover)).toBe(0);
    // A draft's cover is as hidden as the draft.
    await request(h.app).get(`/v1/listings/${listing.id}/cover`).expect(404);

    await request(h.app)
      .put(`/v1/listings/${listing.id}/cover`)
      .set(auth(seller))
      .set("Content-Type", "image/webp")
      .send(webp(1024, 1024))
      .expect(204);
    const again = await request(h.app)
      .get(`/v1/listings/${listing.id}`)
      .set(auth(seller))
      .expect(200);
    expect(again.body.listing.coverUrl).toMatch(/\?v=2$/);

    await request(h.app).delete(`/v1/listings/${listing.id}/cover`).set(auth(seller)).expect(204);
    await request(h.app).get(`/v1/listings/${listing.id}/cover`).set(auth(seller)).expect(404);
    const gone = await request(h.app)
      .get(`/v1/listings/${listing.id}`)
      .set(auth(seller))
      .expect(200);
    expect(gone.body.listing.coverUrl).toBeNull();
  });

  it("rejects other types, empty bodies, unreadable pictures and covers too large", async () => {
    const listing = await createListing(h, seller);
    const put = () => request(h.app).put(`/v1/listings/${listing.id}/cover`).set(auth(seller));
    await put().set("Content-Type", "image/gif").send(Buffer.from("GIF89a")).expect(400);
    await put().set("Content-Type", "image/png").send(Buffer.alloc(0)).expect(400);
    await put().set("Content-Type", "image/png").send(Buffer.from("not a picture")).expect(400);
    await put().set("Content-Type", "image/png").send(jpeg(64, 64)).expect(400);
    await put().set("Content-Type", "image/jpeg").send(jpeg(2048, 1152)).expect(204);
    const wide = await put().set("Content-Type", "image/jpeg").send(jpeg(2049, 64)).expect(413);
    expect(wide.body.error.message).toBe(
      "a cover is at most 2048 pixels a side, this one is 2049 by 64",
    );
    const big = await put()
      .set("Content-Type", "image/png")
      .send(Buffer.concat([png(64, 64), Buffer.alloc(400 * 1024)]))
      .expect(413);
    expect(big.body.error).toEqual({ code: "too_large", message: "a cover is at most 400 KiB" });
  });
});
