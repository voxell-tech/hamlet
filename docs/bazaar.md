# The Agent Bazaar

A market in the world and a panel in the window where the chief brings
things home: models for the catalogue, skills for the library, the
agents, tools and whole hamlets other chiefs sell at the bazaar's
server, where the chief sells their own, and the official looks agents
and hamlets wear ([`looks.md`](looks.md)).

## In the world

The bazaar is a striped pavilion and a ring of six stalls on ground of
its own beside the first town, in no hamlet, raised after the library
(`bazaar::Pitching`). Each stall has a keeper, the pavilion a merchant,
and shoppers stroll between them. Lanterns, fairy lights and the glows
over the avenue come on at dusk, and a purchase is cheered: the merchant
waves, embers rise and the lights flare (`bazaar::Traded`). Small things
are drawn and move only while the camera is near, as a town's robots
are.

The code is `hamlet_realm/src/bazaar.rs` and `bazaar/`: `pitch.rs` stands
it, `build/` makes its meshes, `keepers.rs`, `merchant.rs` and
`shoppers.rs` its people, `lights.rs` and `bustle.rs` what glows and
moves. Its colours are the `awnings`, `fairy` and `stalls` of
`assets/palette.toml`, and its lights and people are tuned under
`[bazaar]` in `assets/world.toml`.

It answers the pointer as `Thing::Bazaar`, and each stall as
`Thing::Stall`. The chief's mascot goes to what is chosen and stands
where a visitor does (`chief::Visitor`). Its right-click menu opens the
panel, at the stall when one is clicked, and moves it: it is picked up
and set down as the library is, and its place is kept in the window's
folder as `bazaar.place` (`bazaar::Pitched`). View > Go to the Bazaar
frames it whole (`pick::frame_round`).

`cargo run --example chief_sheet -- --bazaar --out <dir>` draws it by
day and by night, each stall, a stall chosen, a sale and a move.

## The panel

`hamlet_app/src/screens/bazaar.rs` is the panel, a tab beside the
Inspector's, with a tab of its own for each stall. It opens from the
HAMLET menu (Open the Agent Bazaar, Browse the Models, Browse the
Hamlets, Sell at the Agent Bazaar), from the bazaar or a stall in the world, from Browse the Bazaar
in Settings' models and under an agent's model, and from the setup of
another place for models.

Every stall is one shelf (`bazaar/shelf.rs`): its listings, searched by
words, put in order and narrowed, shown a page of rows at a time, and
one listing on a page of its own. `bazaar/view/parts.rs` is what the
shelves and pages are made of, out of the panels' own looks.

| Stall | What it shows | Where it comes from | What having one does |
|---|---|---|---|
| Models | OpenRouter's models, and those each other endpoint serves | `openrouter.ai/api/v1/models`, the catalogue | `edit` of the catalogue, `allowed`, after a `read` of the models when the server has not listed it; `edit` of the settings for the default |
| Skills | skills.sh's most installed skills, and what a search finds | `skills.sh/api/...` | `take-skill` with its files |
| Agents, Plugins, Hamlets | What other chiefs sell | The bazaar's server; where it cannot be reached, `bazaar/samples/` | Bought there, then `settle` with its package; see [The market](#the-market) |
| Hamlets, as samples | Hamlet templates | `bazaar/samples/hamlets.rs` | `found-from` with its mayor and agents, for nothing |
| Agents, Plugins, as samples | Made up | `bazaar/samples/` | Nothing: the page says it is a sample |
| Looks | The official skins, meshes and emotes agents and hamlets wear | The bazaar's server's catalogue of assets | Bought there; worn with a `looks` edit, its file downloaded for the realm; see [`looks.md`](looks.md) |

OpenRouter's models run through the chief's sign-in with OpenRouter, or
an endpoint at its address. With neither, a model waits for one to be
set up and is added once there is.

The fetches run on the window's Tokio (`bazaar/errand.rs`) and their
answers are taken in on the frame. With no runtime there is no fetch,
and the stall says it has nothing yet. `take-skill` and `found-from` are
in [`protocol.md`](protocol.md#the-agent-bazaar).

A template's page, and that of a hamlet for sale, shows its town turning
on a plinth: the realm stands it apart from the world and draws it into a
picture (`hamlet_realm/src/diorama.rs`, `Dioramas`), only while the
picture is in sight.

A look's page shows it in the viewer: one subject stood apart from the
world in the same way, which the chief turns by dragging and brings nearer
with the wheel, with RESET and BIGGER or SMALLER under it. The realm's
side is `hamlet_realm/src/viewer.rs` (`Viewer`, any `Subject`), the
panel's `hamlet_app/src/viewer.rs`; it draws only while it is in
sight, and turns by itself once left alone for `[viewer] idle` seconds
(`assets/world.toml`).

## The market

The bazaar's server is a separate service: chiefs have a Hamlet account
there, put agents, hamlets and tools up for sale, and buy them through
Stripe. The window reaches it at `HAMLET_BAZAAR_URL`, or
`http://localhost:8787`, on its Tokio, with the session token as a
bearer header (`bazaar/market/client.rs`), which holds each request the
window makes and the answer it expects.

`bazaar/market.rs` owns the market in the window: who is signed in, the
listings, what the chief owns and sells, and what was bought and
settled into the home, which the home's server keeps (a `bought` read).
The AGENTS, PLUGINS and HAMLETS stalls show the server's listings once
they come (`bazaar/listings.rs`), and the samples where it cannot be
reached, said to be samples, with ASK AGAIN at their foot.

- A Hamlet account (`market/account.rs`, its pages in
  `screens/account.rs`), all in the app, never in a browser: CREATE AN
  ACCOUNT takes an email, a username and a password, then the 6-digit
  code emailed (SEND AGAIN for another); SIGN IN takes the email and
  password, and turns to the code while the email is not verified;
  FORGOT PASSWORD sends a code to choose a new one with. First-run
  setup offers the account as its first, skippable step, the bazaar at
  SIGN IN, and Settings on its ACCOUNT page, with CHANGE USERNAME,
  CHANGE PASSWORD and SIGN OUT. The token is kept for the next run in
  `bazaar-token` in the window's view folder, readable by its owner
  alone, with the server it came from, who it signs in and the vault's
  data key, shown signed in before the server answers
  (`market/session.rs`). A token the server refuses signs out.
- The OpenRouter key on every computer (`market/vault.rs`): at sign-in
  the window makes the password key (Argon2id, `hamlet_protocol::vault`)
  and opens the account's vault with it, or makes one. It never sees a
  provider key (S7): it hands the data key to the home, which seals its
  OpenRouter key for the vault (a `sealed` read) or opens the vault's
  into its credentials (an `unseal` login). A key connected later is
  sealed and kept as it comes. Signing out keeps the home's key;
  DISCONNECT OPENROUTER (Settings, ACCOUNT or ENDPOINTS) forgets it on
  the home and in the account. CONNECT OPENROUTER asks first for a
  spend limit, $10 unless changed, which OpenRouter's page is asked to
  fill in.
- A listing's page (`view/wares.rs`, `view/trade.rs`): its cover under
  its head, fetched and kept a few at a time (`bazaar/covers.rs`), or a
  hamlet's town; then the way it is had as it stands: BUY or GET IT FOR
  FREE with where an agent goes, INSTALL when it is owned, UPDATE TO a
  newer version when one is out, CHANGE THIS LISTING when it is the
  chief's. REPORT, on others' listings, sends the bazaar a reason.
- A deal (`market/deal.rs`): a paid listing opens Stripe's page and
  asks the server every five seconds until it is paid; then its package
  is downloaded whole and handed to the home's server with `settle` and
  its parts, whose last answer says what came of it. One deal goes at a
  time. An agent moves into the hamlet chosen, a hamlet is founded and
  shown, tools join those added by hand. The server's side is in
  [`protocol.md`](protocol.md#packages).
- The seller's desk (`view/desk.rs`, `market/sell.rs`): SELL AN AGENT,
  SELL A HAMLET or SELL TOOLS opens a form laid out as Settings is. What
  is sold is packed by the home's server (a `package` read, in parts),
  the listing is put up or changed with its words, price, tags and
  cover, the package is uploaded as its new version, and PUT UP FOR SALE
  lists it. A cover is a PNG, JPEG or WebP of at most 400 KB, from a
  file of this computer or an address on the web. A price starts at
  $1.99 and is at least $0.99. Publishing first shows the seller terms
  to agree to. A paid listing needs payouts set up with Stripe, which
  SET UP PAYOUTS opens after the terms; CHECK AGAIN asks Stripe whether
  the chief can be paid.
- The bazaar's checks (`market/check.rs`): a listing put up, and each
  new version of one on the shelves, is checked before buyers get it.
  The form's ON THE SHELVES says it is being checked, asking the server
  every five seconds until it is done, or why it was rejected, with ASK
  A HUMAN to have a person at the bazaar look at it. The limits on
  selling (a verified email, an account 7 days old, 3 sent a day and 2
  checked at once) are said with what to do (`Refusal::words`).
- YOURS lists what the chief sells, each opening its form, and what they
  bought, each opening its page with how it stands in the home.

The panel's sizes are under `[bazaar]` in `assets/ui.toml`. The
`bazaar-*` scenes of `cargo run --example ui_gallery` draw each stall and
a page of each, with OpenRouter's and skills.sh's answers given by hand
(`bazaar::offer`), and the `market-*` scenes the market signed in and
out, signing in, a listing at each step of a deal, the desk, a listing
rejected by the checks and Yours,
with the bazaar's server's answers given by hand (`bazaar::stage`).
