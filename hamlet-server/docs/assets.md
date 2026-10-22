# hamlet-server: official assets

Paid cosmetics made by the Hamlet team and sold at the bazaar's Asset
stall: emotes, skins and meshes for agents and hamlets. Unlike listings,
these are not made by users and every cent goes to the platform's own
Stripe account (no Connect).

Conventions (auth, errors, ids, money) are those of [`api.md`](api.md).

## Why the files are guarded

The Hamlet client may be open source. A paid asset's file is therefore
never in the client's repository and never public: it is sent only to a
signed-in owner, and the client keeps it in memory. The store shows an
asset through **preview frames**: PNG pictures of it rendered from evenly
spaced angles, which the client spins through as the chief drags. No mesh
is sent to show an asset in the store.

## Objects

### Asset

```json
{
  "id": "agent-skin-sakura",
  "target": "agent",
  "category": "skin",
  "title": "Sakura",
  "summary": "Soft cherry-blossom pinks for your agent.",
  "priceCents": 99,
  "currency": "usd",
  "version": "1.0.0",
  "frames": 24,
  "previewUrl": "/v1/assets/agent-skin-sakura/preview/{frame}?v=1.0.0",
  "owned": false,
  "updatedAt": "..."
}
```

- `id` is a stable slug: lowercase letters, digits and `-`, at most 64.
- `target`: `agent` or `hamlet`. `category`: `emote`, `skin` or `mesh`.
- `priceCents`: `0` (free) or `50..=50000`.
- `frames`: how many preview frames there are, `0..=72`, numbered from 0.
  Frame `i` shows the asset turned `i * 360 / frames` degrees; for an
  emote, the emote `i / frames` of the way through, the view turning a
  little the while.
  `previewUrl` has `{frame}` for the client to fill in.
- `owned` is false without auth. A free asset is owned once claimed
  (`POST .../purchase`), like a free listing.
- Only `listed` assets appear (see Publishing).

## Routes

`GET /v1/assets` -> `200 { "items": [Asset] }`, every listed asset, by
target, then category, then price, then title. Optional query `target`,
`category`.

`GET /v1/assets/:id` -> `200 Asset`, `404` when not listed.

`GET /v1/assets/:id/preview/:frame` -> `image/png`, public, cacheable
(`Cache-Control: public, max-age=86400`). `404` for an unknown asset or a
frame out of range.

`POST /v1/assets/:id/purchase` **auth**

- free: `200 { "owned": true }`
- paid: `200 { "owned": false, "purchaseId": "ap_...", "checkoutUrl": "..." }`,
  a Stripe Checkout Session in `payment` mode on the platform's own account
  (no `transfer_data`, no `application_fee_amount`), with
  `metadata.assetPurchaseId`, success URL `/checkout/success` and cancel URL
  `/checkout/cancel`.
- `409 conflict` when already owned. `404` when not listed.

`GET /v1/asset-purchases/:id` **auth**, buyer only ->
`200 { "id", "assetId", "status": "pending" | "paid" | "expired" }`.

`GET /v1/me/assets` **auth** -> `200 { "items": [Asset] }`, every asset the
user owns, with `owned: true`, including ones no longer listed.

`GET /v1/assets/:id/file` **auth**, owners only (`403` otherwise) -> the
asset's file, byte for byte as published, with its content type
(`model/gltf-binary` for `.glb`, `application/json` for JSON) and headers
`Cache-Control: private, no-store` and `X-Asset-Version: <version>`. An
owner keeps access to an asset that is later unlisted.

## Webhook

`checkout.session.completed` (paid) and
`checkout.session.async_payment_succeeded` whose metadata has
`assetPurchaseId` mark that asset purchase `paid`;
`checkout.session.expired` and `checkout.session.async_payment_failed`
mark it `expired` when still pending. Listing purchases are unaffected.

## Publishing

Assets are published by the team, not through the API. The baker
(`assets-src/`) writes a folder:

```
assets-src/out/
  catalog.json        [{ id, target, category, title, summary, priceCents,
                         version, file, contentType, frames }]
  files/<id>.<ext>    the asset's file
  previews/<id>/<n>.png
```

`pnpm assets:publish [--local|--remote] [--dir assets-src/out]` uploads
every file and preview to R2 (`assets/<id>/<version>/file`,
`assets/<id>/<version>/preview/<n>.png`) and upserts each catalog entry in
D1 with status `listed`. An asset in D1 that is missing from the catalog
is set `unlisted`, never deleted, so owners keep it. Publishing is
idempotent.
