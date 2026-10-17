# hamlet-server API

The marketplace behind Hamlet's Agent Bazaar. Chiefs (Hamlet users) sign in
with GitHub, put agents, hamlets and plugins up for sale, and buy those of
others through Stripe. Everything is JSON over HTTPS unless said otherwise.
The official assets the platform sells itself are in [`assets.md`](assets.md).

## Conventions

- Base path for the API is `/v1`. Pages a browser is sent to (Stripe and
  payout returns) are at the root.
- Auth: `Authorization: Bearer <token>`, a session token from the device
  sign-in. Routes marked **auth** answer `401` without one.
- Errors: any status >= 400 has the body
  `{ "error": { "code": string, "message": string } }`.
  Codes used: `bad_request`, `unauthorized`, `forbidden`, `not_found`,
  `conflict`, `too_large`, `payouts_required`, `pending`, `slow_down`,
  `expired`, `denied`, `internal`.
- Times are ISO 8601 strings. Money is integer cents, currency always `usd`.
- Ids are opaque strings.

## Kinds

`kind` is one of `agent`, `hamlet`, `plugin`.

## Prices

`priceCents` is `0` (free) or between `50` and `50000` inclusive. A paid sale
sends 90% to the seller's connected Stripe account; the platform keeps
`round(priceCents * PLATFORM_FEE_PERCENT / 100)` (default 10%) as Stripe's
`application_fee_amount`. A free item is owned at once with no Stripe at all.

## Ownership

Buying gives a lasting right to every version of that listing. Unlisting
does not take it away: an owner still sees the listing, its versions and can
download them. Only an admin takedown (`removed`) stops downloads.

Updates are never pushed: the client chooses which version to download.

## Objects

### User

```json
{
  "id": "u_...",
  "login": "octocat",
  "name": "The Octocat",
  "avatarUrl": "https://avatars.githubusercontent.com/u/1?v=4",
  "payouts": { "connected": true, "enabled": true }
}
```

`payouts` appears only on `GET /v1/me`.

### Seller

```json
{ "id": "u_...", "login": "octocat", "name": "The Octocat", "avatarUrl": "...", "verified": true }
```

`verified` is true when the seller's Stripe account can take payments.

### Listing

```json
{
  "id": "l_...",
  "kind": "agent",
  "title": "PR Reviewer",
  "summary": "Reviews pull requests for bugs and missing tests.",
  "description": "Longer text, markdown.",
  "category": "Code",
  "tags": ["github", "review"],
  "priceCents": 900,
  "currency": "usd",
  "status": "listed",
  "seller": { "...": "Seller" },
  "coverUrl": "/v1/listings/l_.../cover?v=3",
  "latest": { "...": "Version, or null when none uploaded" },
  "owners": 12,
  "stars": 4.6,
  "reviews": 5,
  "createdAt": "...",
  "updatedAt": "..."
}
```

- `status`: `draft` (never published), `listed`, `unlisted`, `removed`.
- `coverUrl` is null when there is no cover. It is relative to the server.
- `stars` is the mean of review stars, `0` with no reviews.
- `owners` counts paid (or free) purchases.
- `updatedAt` moves when the listing's text, price or versions change.

Limits, checked by the server:

| Field | Rule |
|---|---|
| `title` | 1 to 60 chars, trimmed |
| `summary` | 1 to 140 chars |
| `description` | 0 to 5000 chars |
| `category` | 1 to 32 chars |
| `tags` | at most 8, each 1 to 24 chars, lowercased, unique |
| `priceCents` | `0` or `50..=50000` |

### Version

```json
{
  "version": "1.2.0",
  "changes": "What changed, in a line or a few.",
  "size": 18234,
  "sha256": "hex",
  "createdAt": "...",
  "meta": { "...": "kind specific, below" }
}
```

`version` is `MAJOR.MINOR.PATCH` (digits only) and must be greater than the
listing's previous one. `changes` is 0 to 2000 chars. `size` is the byte
length of the stored package.

`meta` is derived by the server from the uploaded package, never sent by the
seller, so the store shows what is really inside:

- agent: `{ "name", "purpose", "model": string|null, "character": string (CHARACTER.md, cut to 4000 chars), "skills": [string], "notes": bool }`
- hamlet: `{ "name", "title", "kind", "purpose", "seed": string, "mayor": Settler, "agents": [Settler] }`
  where Settler is `{ "name", "purpose", "model": string|null, "character": string (cut to 1000 chars), "skills": [string], "notes": bool }`
- plugin: `{ "tools": [{ "name", "description", "permissions": [string] }] }`
  with permissions from each tool's `manifest.json`: always `run_program`;
  `network` when `sandbox.net` is true; `write_files` when `sandbox.fs_rw`
  is a non-empty array; `read_files` when `sandbox.fs_ro` is a non-empty
  array.

`skills` are the folder names directly under `workspace/skills/`.
`notes` is true when `workspace/NOTES.md` is in the package.

### Review

```json
{ "by": { "login": "...", "avatarUrl": "..." }, "stars": 5, "words": "...", "createdAt": "..." }
```

## Packages

A package is one JSON document, at most 10 MB as sent (`MAX_PACKAGE_BYTES`,
default 10485760). It is stored in R2 byte for byte as received and served
back the same way.

A file in a package:

```json
{ "path": "workspace/skills/review/SKILL.md", "data": "<base64>", "executable": false }
```

`path` is relative, `/`-separated, with no empty, `.` or `..` segment, no
leading `/`, no `\`, at most 200 chars. Paths are unique within one file list.
`executable` is optional, default false. `data` must be valid base64.

Names (agent names, hamlet name, tool names, skill folder names) match
`^[a-z0-9][a-z0-9_-]{0,31}$`.

### agent

```json
{
  "format": 1,
  "kind": "agent",
  "agent": {
    "name": "pr-reviewer",
    "purpose": "Reviews pull requests.",
    "model": "anthropic/claude-sonnet-4" ,
    "files": [ File ]
  }
}
```

Files are relative to the agent's house. Allowed: `CHARACTER.md`,
`workspace/NOTES.md`, and anything under `workspace/skills/<skill>/`.
`model` is optional (string or null); a suggestion only. `purpose` is 1 to
500 chars.

### hamlet

```json
{
  "format": 1,
  "kind": "hamlet",
  "hamlet": {
    "name": "launch-crew",
    "title": "Launch Crew",
    "kind": "SaaS launch team",
    "purpose": "Ships a small product.",
    "seed": "1234567890123456789",
    "mayor": Agent,
    "agents": [ Agent ]
  }
}
```

`Agent` is the `agent` object of an agent package. `seed` is a decimal
string (it is a u64 in Hamlet, too big for a JS number). Agent names are
unique within the hamlet. `title` is 1 to 32 chars, `kind` 1 to 60.
At most 16 agents besides the mayor.

### plugin

```json
{
  "format": 1,
  "kind": "plugin",
  "plugin": {
    "tools": [ { "name": "calendar", "files": [ File ] } ]
  }
}
```

Each tool's files are relative to `tools/<name>/` in the buyer's home and
must include `manifest.json`, a JSON object whose `name` equals the tool's
`name` and whose `exec` is a non-empty array of strings. `description` is
optional. 1 to 16 tools.

The package's `kind` must equal the listing's.

## Routes

### Health

`GET /health` -> `200 { "ok": true }`

### Sign in (GitHub device flow, proxied)

`POST /v1/auth/device` -> `200`

```json
{ "deviceCode": "...", "userCode": "ABCD-1234", "verificationUri": "https://github.com/login/device", "interval": 5, "expiresIn": 900 }
```

`POST /v1/auth/device/poll` with `{ "deviceCode": "..." }`

- not approved yet: `428` code `pending`
- polling too fast: `429` code `slow_down` (client adds 5 s to its interval)
- `410` code `expired`, `403` code `denied`
- approved: `200 { "token": "hms_...", "user": User }`

The server upserts the user by GitHub id (login, name and avatar refreshed on
each sign-in). Tokens last 90 days; only their SHA-256 is stored.

`DELETE /v1/auth/session` **auth** -> `204`. Revokes the token used.

### Me

`GET /v1/me` **auth** -> `200 User` (with `payouts`). If connected and not
yet enabled, the server asks Stripe for the account's state first.

`POST /v1/me/payouts` **auth** -> `200 { "url": "https://connect.stripe.com/..." }`.
Creates a Stripe Express account for the user when they have none, and
returns an onboarding link to open in a browser. Its return and refresh
URLs are `/payouts/return` and `/payouts/refresh` on this server.

`GET /v1/me/listings` **auth** -> `200 { "items": [Listing] }`, the user's
own listings in every status, newest first.

`GET /v1/me/library` **auth** -> `200 { "items": [ { "listing": Listing, "versions": [Version], "ownedSince": "..." } ] }`.
Everything the user owns, including unlisted listings, excluding `removed`.
`versions` newest first.

### Browse

`GET /v1/listings` -> `200 { "items": [Listing], "total": n }`

Query: `kind` (optional), `q` (words; every word must appear,
case-insensitively, in the title, summary, category, tags or the seller's
login), `order` = `popular` (default, owners desc) | `rated` (stars desc,
then reviews desc) | `cheapest` (price asc) | `newest` (updatedAt desc), ties
by title; `limit` 1..100 (default 50); `offset` (default 0).

Only `listed` listings with at least one version appear.

`GET /v1/listings/:id` -> `200`

```json
{ "listing": Listing, "versions": [Version], "reviews": [Review], "owned": false, "mine": false }
```

Visible when `listed`; otherwise only to its seller and its owners (and
admins). `removed` is visible only to the seller and admins. Otherwise
`404`. `owned`/`mine` are false without auth. Reviews newest first, at most
50.

`GET /v1/listings/:id/cover` -> image bytes with its content type, when the
listing is visible as above; `404` if it has none.

### Sell

`POST /v1/listings` **auth** with `{ kind, title, summary, description?, category, tags?, priceCents }`
-> `201 Listing` with status `draft`.

`PATCH /v1/listings/:id` **auth**, seller only, any of `{ title, summary, description, category, tags, priceCents }`
-> `200 Listing`. Not allowed on `removed` (`403`). Changing to a paid price
needs payouts enabled (`403 payouts_required`) when the listing is `listed`.

`PUT /v1/listings/:id/cover` **auth**, seller only. Raw body, `Content-Type`
one of `image/png`, `image/jpeg`, `image/webp`; at most 2 MB -> `204`.

`DELETE /v1/listings/:id/cover` **auth**, seller only -> `204`.

`POST /v1/listings/:id/versions` **auth**, seller only, with
`{ "version": "1.0.0", "changes": "...", "package": Package }` -> `201 Version`.
`400` when the package is invalid (the message says what), `413 too_large`
over the limit, `409` when the version is not greater than the last.

`POST /v1/listings/:id/publish` **auth**, seller only -> `200 Listing` with
status `listed`. Needs at least one version (`400`), and payouts enabled
when paid (`403 payouts_required`). Not from `removed`.

`POST /v1/listings/:id/unlist` **auth**, seller only -> `200 Listing` with
status `unlisted`.

### Download

`GET /v1/listings/:id/versions/:version/package` **auth** -> the package
JSON. For the seller, owners and admins; not when `removed` (except admins).

### Buy

`POST /v1/listings/:id/purchase` **auth**

- free: `200 { "owned": true }`
- paid: `200 { "owned": false, "purchaseId": "p_...", "checkoutUrl": "https://checkout.stripe.com/..." }`
- `409 conflict` when already owned, `400` when buying one's own listing,
  `404` when not `listed`.

Paid: a Stripe Checkout Session in `payment` mode with one line item
(`price_data` from the listing's title and price), `payment_intent_data` with
`application_fee_amount` and `transfer_data.destination` = the seller's
account, `metadata.purchaseId`, success URL `/checkout/success` and cancel
URL `/checkout/cancel`. The purchase is `pending` until the webhook says
paid. A second purchase call while one is pending makes a new session.

`GET /v1/purchases/:id` **auth**, buyer only ->
`200 { "id", "listingId", "status": "pending" | "paid" | "expired" }`.
While it is pending, the server asks Stripe for its Checkout Session and
settles it as the webhook would, so a missed or late webhook does not
leave it pending.

### Reviews

`POST /v1/listings/:id/reviews` **auth**, owners only (not the seller), with
`{ "stars": 1..5, "words": 0..1000 chars }` -> `200 Review`. One per user per
listing; posting again replaces it.

### Stripe webhook

`POST /v1/stripe/webhook`. Raw body, verified with `STRIPE_WEBHOOK_SECRET`
(`400` when the signature is wrong). Handles:

- `checkout.session.completed` with `payment_status` `paid`, and
  `checkout.session.async_payment_succeeded`: purchase -> `paid`.
- `checkout.session.expired`, `checkout.session.async_payment_failed`:
  purchase -> `expired` (when still pending).
- `account.updated`: the seller's `payoutsEnabled` = `charges_enabled`.

Unknown events answer `200`. Handling is idempotent.

### Browser pages

`GET /checkout/success`, `GET /checkout/cancel`, `GET /payouts/return`,
`GET /payouts/refresh`: small HTML pages telling the user to go back to
Hamlet. `/payouts/refresh` makes a fresh onboarding link only when called
with a valid session, so it just says to press the button in Hamlet again.

### Admin

Admins are the GitHub logins in `ADMIN_GITHUB_LOGINS` (comma separated).

`POST /v1/admin/listings/:id/takedown` **auth**, admin, `{ "reason": "..." }`
-> `200 Listing` with status `removed`.

`POST /v1/admin/listings/:id/restore` **auth**, admin -> `200 Listing` with
status `unlisted`.
