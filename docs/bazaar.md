# The Agent Bazaar

A market in the world and a panel in the window where the chief brings
things home: models for the catalogue, skills for the library and whole
hamlets. Agents to hire and plugins to install are shown too, and come
with the marketplace.

## In the world

The bazaar is a striped pavilion and a ring of five stalls on ground of
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
Hamlets), from the bazaar or a stall in the world, from Browse the Bazaar
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
| Hamlets | Hamlet templates | `bazaar/samples/hamlets.rs` | `found-from` with its mayor and agents |
| Agents, Plugins | What others made | `bazaar/samples/` | Nothing yet: the page says it comes with the marketplace |

OpenRouter's models run through the chief's sign-in with OpenRouter, or
an endpoint at its address. With neither, a model waits for one to be
set up and is added once there is.

The fetches run on the window's Tokio (`bazaar/errand.rs`) and their
answers are taken in on the frame. With no runtime there is no fetch,
and the stall says it has nothing yet. `take-skill` and `found-from` are
in [`protocol.md`](protocol.md#the-agent-bazaar).

A template's page shows its town turning on a plinth: the realm stands
it apart from the world and draws it into a picture
(`hamlet_realm/src/diorama.rs`, `Dioramas`), only while the picture is in
sight.

The panel's sizes are under `[bazaar]` in `assets/ui.toml`. The
`bazaar-*` scenes of `cargo run --example ui_gallery` draw each stall and
a page of each, with OpenRouter's and skills.sh's answers given by hand
(`bazaar::offer`).
