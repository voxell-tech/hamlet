# hamlet-server

The marketplace behind Hamlet's Agent Bazaar: chiefs sign in with GitHub,
sell agents, hamlets and plugins, and buy them through Stripe.

- The HTTP API is specified in [`docs/api.md`](docs/api.md). That file is the
  contract with the Hamlet client, with [`docs/assets.md`](docs/assets.md)
  for the official assets.
- Express 5 running on Cloudflare Workers, D1 (SQLite) through Prisma for
  data, R2 for package and cover files, Stripe Connect for payments.

## Layout

| Path                                           | What                                                                                                   |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `src/app.ts`                                   | `createApp(services)`: the whole API, with no global state                                             |
| `src/index.ts`                                 | The Worker entry: builds the services from the environment                                             |
| `src/routes/`                                  | One file per area: auth, me, browse, sell, download, purchases, reviews, assets, webhook, admin, pages |
| `src/package/`                                 | Package validation (`schema.ts`), meta derivation (`meta.ts`)                                          |
| `src/assets.ts`, `src/assetKeys.ts`            | Official assets: their JSON and ownership, and their R2 keys                                           |
| `src/money.ts`                                 | Prices and the platform fee                                                                            |
| `src/stripe.ts`, `src/github.ts`               | The real Stripe and GitHub clients                                                                     |
| `prisma/schema.prisma`                         | The database schema, source of truth                                                                   |
| `migrations/`, `migrations-down/`              | Each migration and its undo: `0001_init`, `0002_add_assets`                                            |
| `scripts/db.ts`                                | The `db:*` commands                                                                                    |
| `scripts/assets.ts`, `scripts/assetCatalog.ts` | `assets:publish`, and its catalog checks and SQL                                                       |
| `scripts/seed.ts`, `scripts/mockData.ts`       | `db:seed`, and its listings, packages and SQL                                                          |
| `seed/mock.json`                               | The mock bazaar `db:seed` puts on a local server                                                       |
| `test/`                                        | Vitest tests, run in Node against Miniflare's local D1 and R2                                          |

## Setup guide

### 1. Prerequisites

- Node 24 and pnpm.
- A Cloudflare account (free is enough to start; see "Limits" below).
- A GitHub account, for the OAuth App.
- A Stripe account, and the [Stripe CLI](https://docs.stripe.com/stripe-cli)
  for local webhooks.

### 2. Install

```sh
cd hamlet-server
pnpm install          # also generates the Prisma client (pnpm db:generate redoes it)
cp .dev.vars.example .dev.vars
```

`pnpm exec wrangler login` signs Wrangler in to your Cloudflare account. It
is needed for the remote steps below, not for local development or tests.

### 3. D1 database and R2 bucket

```sh
pnpm exec wrangler d1 create hamlet-bazaar
pnpm exec wrangler r2 bucket create hamlet-bazaar-blobs
```

The first command prints a `database_id`. Paste it into `wrangler.jsonc`
in place of `REPLACE_WITH_D1_DATABASE_ID`. Keep the names as they are, or
change them in `wrangler.jsonc` too.

### 4. GitHub OAuth App (sign-in)

1. GitHub > Settings > Developer settings > OAuth Apps > New OAuth App.
2. Application name: `Hamlet Bazaar`. Homepage URL and Authorization
   callback URL: your `PUBLIC_URL` (the callback is not used by the device
   flow, but GitHub asks for one).
3. Register, then on the app's page check **Enable Device Flow** and save.
4. Copy the **Client ID** into `.dev.vars` as `GITHUB_CLIENT_ID`. No client
   secret is needed: the device flow does not use one.

### 5. Stripe

Do all of this in **test mode** first.

1. Dashboard > Developers > API keys: copy the secret key (`sk_test_...`)
   into `.dev.vars` as `STRIPE_SECRET_KEY`.
2. Connect > Get started: choose the **platform or marketplace** model with
   **Express** accounts, and fill in the platform profile Stripe asks for.
   Sellers' payouts go through these Express accounts; the platform keeps
   `PLATFORM_FEE_PERCENT` (10 by default) of each sale.
3. Local webhooks: in another terminal run

   ```sh
   stripe login
   stripe listen --forward-to localhost:8787/v1/stripe/webhook \
     --events checkout.session.completed,checkout.session.async_payment_succeeded,checkout.session.expired,checkout.session.async_payment_failed,account.updated
   ```

   and copy the `whsec_...` it prints into `.dev.vars` as
   `STRIPE_WEBHOOK_SECRET`. It forwards both your account's and connected
   accounts' events.

4. Production webhooks: Developers > Webhooks > Add destination, URL
   `<PUBLIC_URL>/v1/stripe/webhook`. Make two destinations to the same URL:
   - events from **your account**: `checkout.session.completed`,
     `checkout.session.async_payment_succeeded`, `checkout.session.expired`,
     `checkout.session.async_payment_failed`;
   - events from **connected accounts**: `account.updated`.

   Each has its own signing secret. Set `STRIPE_WEBHOOK_SECRET` to both,
   comma separated: `whsec_aaa,whsec_bbb`.

### 6. Where settings go

| Name                    | Kind   | Local            | Production                                            |
| ----------------------- | ------ | ---------------- | ----------------------------------------------------- |
| `STRIPE_SECRET_KEY`     | secret | `.dev.vars`      | `pnpm exec wrangler secret put STRIPE_SECRET_KEY`     |
| `STRIPE_WEBHOOK_SECRET` | secret | `.dev.vars`      | `pnpm exec wrangler secret put STRIPE_WEBHOOK_SECRET` |
| `GITHUB_CLIENT_ID`      | secret | `.dev.vars`      | `pnpm exec wrangler secret put GITHUB_CLIENT_ID`      |
| `PUBLIC_URL`            | var    | `wrangler.jsonc` | `wrangler.jsonc`                                      |
| `PLATFORM_FEE_PERCENT`  | var    | `wrangler.jsonc` | `wrangler.jsonc`                                      |
| `MAX_PACKAGE_BYTES`     | var    | `wrangler.jsonc` | `wrangler.jsonc`                                      |
| `ADMIN_GITHUB_LOGINS`   | var    | `wrangler.jsonc` | `wrangler.jsonc`                                      |

`.dev.vars` is git-ignored; never commit real keys. A value in `.dev.vars`
overrides the one in `wrangler.jsonc` for `pnpm dev`, so once
`wrangler.jsonc` holds the production `PUBLIC_URL`, put
`PUBLIC_URL=http://localhost:8787` in `.dev.vars`. After editing `vars` in
`wrangler.jsonc`, run `pnpm cf-typegen` to refresh
`worker-configuration.d.ts`.

`ADMIN_GITHUB_LOGINS` is a comma-separated list of GitHub logins that may
take listings down and restore them, for example `"octocat,hubot"`.

### 7. Run it locally

```sh
pnpm db:up            # create the tables in the local D1 (.wrangler/)
pnpm dev              # http://localhost:8787
curl localhost:8787/health
```

Local D1 and R2 live in `.wrangler/state`; delete that folder to start over.
For listings to look at, `pnpm db:seed` adds the old mock bazaar (see
"Mock data for local development").

### 8. Point Hamlet at the server

Hamlet looks for the server in this order:

1. the `HAMLET_BAZAAR_URL` environment variable;
2. `bazaar_url` in `~/.hamlet/config.toml`, for example
   `bazaar_url = "https://hamlet-server.<your-subdomain>.workers.dev"`;
3. `http://localhost:8787`.

In Hamlet, the bazaar's AGENTS, PLUGINS and HAMLETS tabs read from the
server. SIGN IN WITH GITHUB opens `github.com/login/device` and shows the
code to enter there. The session token is kept in the OS keychain under
`hamlet` / `bazaar_token`, or in `~/.hamlet/bazaar_token` where there is
no keychain.

### 9. Try it end to end, in test mode

1. Terminal 1: `pnpm db:up && pnpm dev`.
2. Terminal 2: the `stripe listen` command of step 5.3
   (its `whsec_...` must be the one in `.dev.vars`; restart `pnpm dev` if
   you change it).
3. Terminal 3, from the repository root, with a scratch home:
   `HAMLET_HOME=/tmp/hamlet-try cargo run`.
4. Open the bazaar, go to AGENTS and press SIGN IN WITH GITHUB.
5. Press SELL, choose an agent, fill in the title and summary, leave the
   price empty (free) and press PUT UP FOR SALE. It appears on the shelf.
6. For a paid listing: press SET UP PAYOUTS, finish Stripe's test
   onboarding (Stripe offers test data for every field), press CHECK
   AGAIN, then give a price and put it up.
7. To buy, sign in from a second scratch home
   (`HAMLET_HOME=/tmp/hamlet-try-2 cargo run`) as another GitHub account,
   press BUY and pay with the card `4242 4242 4242 4242`, any future date
   and any CVC. Once the webhook arrives, Hamlet downloads and installs it.

### 10. Deploy

1. In `wrangler.jsonc`, set `PUBLIC_URL` to the Worker's URL (for example
   `https://hamlet-server.<your-subdomain>.workers.dev`) and fill in
   `ADMIN_GITHUB_LOGINS`.
2. Set the three secrets with `wrangler secret put` (table above).
3. `pnpm db:up --remote` to create the tables in the real D1.
4. `pnpm run deploy` (plain `pnpm deploy` is a different, built-in pnpm command).
5. Make the production webhook destinations (step 5.4) with the deployed
   URL, and switch Stripe to live keys only once everything works in test
   mode.

To check the bundle without deploying: `pnpm bundle` (a `wrangler deploy
--dry-run` into `dist/`).

## Official assets

The emotes, skins and meshes the Hamlet team sells at the Asset stall
([`docs/assets.md`](docs/assets.md)). They are not listings: the platform
sells them on its own Stripe account, and they are published from the
command line, not through the API.

```sh
# from the repository root: bake the catalog, files and preview frames
cargo run -p hamlet_assets_src --release -- --out hamlet-server/assets-src/out
# then, in hamlet-server
pnpm assets:publish --local     # into .wrangler/state, for pnpm dev
pnpm assets:publish --remote    # into the real R2 bucket and D1
```

`assets:publish` reads `assets-src/out/` (another folder with `--dir`,
relative to where pnpm runs), checks `catalog.json`, uploads every file and
preview to the R2 bucket of `wrangler.jsonc`, and upserts each asset in D1
as listed. An asset missing from the catalog is unlisted, never deleted, so
its owners keep it. Running it again changes nothing. Preview frames are
public; an asset's file is only ever sent to a signed-in owner.

Paid assets use the same webhook destination (events from **your account**)
as listings; no Connect account is involved.

## Mock data for local development

`pnpm db:seed` fills a local server with the mock bazaar Hamlet showed
before it had a server (`seed/mock.json`, taken from the old client): 12
agents, 13 plugins and 6 hamlets, so the AGENTS, PLUGINS and HAMLETS
stalls have something on them.

```sh
pnpm dev                  # in another terminal; the seed goes through its API
pnpm db:seed              # adds whatever is missing; running it again changes nothing
pnpm db:seed --reset      # removes all mock data first, then seeds again
```

- Every seller is a mock user (GitHub id `mock-<login>`) with a session
  whose token is `hms_mock_<login>`. Sellers of paid listings get a fake
  Stripe account (`acct_mock_<login>`) with payouts on, so the store shows
  them as verified.
- Listings, versions and publishing go through the HTTP API, so packages
  are checked and their meta derived as for anyone. A plugin's old
  changelog becomes its versions.
- Each listing gets up to three purchases and reviews by mock reviewers
  (`mock-reviewer-<n>`), and its owners, reviews and stars are set to the
  old numbers, which are far more than those rows. They are recounted from
  the rows as soon as a real purchase or review happens on that listing.
- Free mock listings can be got as usual. Buying a paid one cannot
  complete: the seller's Stripe account is fake.
- `--reset` removes only mock users and what is theirs: their listings
  with the listings' package and cover files in R2, their purchases,
  reviews and sessions. A mock listing a real user got or reviewed is
  kept, with its seller.
- It is for local development: it refuses `--remote` unless given
  `--remote --yes --url <server>`. Mock data does not belong in production.

## Migrations

`prisma/schema.prisma` is the source of truth. Wrangler and Prisma only
migrate up, so each migration here comes with its undo.

```sh
pnpm db:new add_bundles   # after editing schema.prisma
pnpm db:status            # applied / pending
pnpm db:up                # apply pending (add --remote for production)
pnpm db:down              # undo the newest applied one (add --remote)
```

`db:new <name>` diffs the schema snapshot of the previous migration
(`prisma/snapshots/`) against `schema.prisma` and writes
`migrations/NNNN_<name>.sql` (up), `migrations-down/NNNN_<name>.sql` (the
reverse diff) and a new snapshot. Commit all three. Read both SQL files
before applying them.

D1 always enforces foreign keys, and Prisma's SQLite scripts assume they
can be switched off. `scripts/d1Sql.ts` rewrites each generated script so
it is safe on D1: tables are dropped children first, and when a table that
others point at has to be rebuilt (SQLite's way of changing a column), the
rows of the tables pointing at it are set aside and put back, instead of
being deleted by the cascade. `db:new` says so when that happens.

`db:down` runs the down file of the newest row in Wrangler's
`d1_migrations` table, then deletes that row. Do not edit a migration that
has been applied anywhere; make a new one.

## Checks

```sh
pnpm typecheck
pnpm lint
pnpm test        # Node + Miniflare; no network, no real Stripe or GitHub
pnpm format      # prettier --write
pnpm bundle      # wrangler deploy --dry-run
```

Tests get a fresh in-memory D1 and R2 from Miniflare per test file, apply
every migration, and drive `createApp` with supertest. Stripe and GitHub
are in-memory fakes, except webhook signatures, which go through Stripe's
real verification with a test secret.

## Limits

- Workers Free allows 10 ms of CPU per request. Checking a large package
  upload takes more, so use the Workers Paid plan for real traffic.
- A package is at most `MAX_PACKAGE_BYTES` (10 MB) as sent; a cover at
  most 2 MB.
