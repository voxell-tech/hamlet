# The home

Everything Hamlet keeps is in one folder, `~/.hamlet`. `HAMLET_HOME` moves
it elsewhere.

```
~/.hamlet/
  config.toml     settings and endpoints (keys are in the keychain, or in api_key.<endpoint>)
  models.toml     the models you added, with what each costs
  bazaar.json     what OpenRouter and the endpoints offer, asked again each day; safe to delete
  skills_sh.json  the skills skills.sh lists, asked again each day; safe to delete
  bought.json     what was bought at the bazaar's server and written into the home, at which version
  library/        the skills every agent shares, one folder each
  library.jsonl   what was shelved there, refused and taken out
  charter.md      your own charter, the words every agent's prompt opens with; absent until you edit it
  CHARACTER.md    the mayor's character, if you gave it one; no tool of its own can write it
  workspace/      the only place the mayor's tools can write, besides a folder its hamlet works on
    NOTES.md      its memory, shown to it every turn
    skills/       its skills, one folder each with a SKILL.md
  tools/          tools added by hand; read-only to the agent
  sessions/       the mayor's logs, one JSONL file per session
  hamlet.jsonl    the record of the hamlet: agents, roads, mayors, votes
  realm.jsonl     the record between hamlets: founding, chiefs, roads
  limits.toml     the chief's limits on agents, roads, spending and tools
  oversight.jsonl what waited for the chief and how it was answered
  agents/<name>/  an agent's house, laid out like the home itself: workspace/, sessions/, CHARACTER.md
  archive/<id>/   the house of an agent that was removed or merged
```

The records are append-only and are the source of truth: the app rebuilds
what it shows from them. [`core.md`](core.md) follows them from the files
to the entities.

## Settings

The endpoints, their API keys and the default model are set in Settings and
stored in `config.toml`, the keys in the OS keychain. Any server that
speaks the OpenAI chat protocol is an endpoint, and a model is named
`endpoint::model`.

## The Agent Bazaar

The Agent Bazaar (HAMLET menu, or Browse the Bazaar under an agent's
model) shows every model OpenRouter offers, with its price, context,
abilities and benchmark scores, and the models of each other endpoint.
OpenRouter's list needs no key and is kept in `bazaar.json` for a day.
In the world the bazaar is a market beside the first town: a
merchant's pavilion and five stalls round it, each with its keeper.
Select the pavilion to see the models the home bought; select a stall
and the Inspector shows what it sells, the same page as that stall's
tab in the bazaar's panel. Far from the camera the bazaar is drawn
plain and its small things are hidden, as towns and robots are. Where a chief set it down is kept in
`bazaar.place`.

| Stall | Keeper | Sells | Buying |
|---|---|---|---|
| AI Models | Brannoc | Models, as below | Adds the model to `models.toml` |
| Community Agents | Wren Halloway | Agents other chiefs put up at the bazaar's server | Moves the agent into the hamlet chosen, with its character, notes and skills |
| Plugins | Tobias Cogsworth | Tools other chiefs put up at the bazaar's server | Adds the tools to the house of the agent chosen, under its `tools/` |
| Skills | Sister Marjoram | Skills listed on skills.sh | Shelves the skill in `library/`, recorded in `library.jsonl` with `from` its source; no librarian is asked, since the chief chose it |
| Hamlet Templates | Lady Ashgrove | Hamlets other chiefs put up at the bazaar's server, each shown as a turning diorama | Founds the hamlet: its mayor, its agents and their characters, notes and skills |

Adding a model writes it to `models.toml`, allowed; one from OpenRouter
first signs the chief in to OpenRouter when the home has no endpoint
there. Only added models are in `models.toml`: List Models updates
their prices and drops those an endpoint no longer serves.


## Buying and selling at the bazaar's server

Community Agents, Plugins and Hamlet Templates sell what chiefs made, from
the bazaar's server, [`hamlet-server`](../hamlet-server/README.md), whose
routes are in [`hamlet-server/docs/api.md`](../hamlet-server/docs/api.md).
The server is `HAMLET_BAZAAR_URL`, else `bazaar_url` in `config.toml`,
else `http://localhost:8787`.

A chief signs in with GitHub from any of the three stalls; the token is
kept like an API key, under `bazaar_token`. Paid listings are paid on
Stripe's page in the browser; the seller receives 90% and the bazaar
keeps 10%. A free one is owned at once.

What is bought is the chief's for good, every version of it, even once
its seller takes it off the shelves. It is written into the home only
when the chief says so, and a newer version only when they press UPDATE:

| Bought | Written | An update |
|---|---|---|
| An agent | Into the hamlet chosen, as a new agent. A name taken in the realm gets `-ii`, `-iii`... | Replaces its `CHARACTER.md` and the skills it came with; its `NOTES.md` stays its own |
| A hamlet | Founded in the realm, as with the realm's limits | Each of its agents as an agent's update; new ones move in |
| A plugin | Its tools into `tools/` of the house of the agent chosen | Replaces those tools |

A package is checked before anything is written: every path stays in the
folder it goes to, and every name is one the realm takes.
`bought.json` keeps, for each listing written into the home, its version,
the names it lives under and where it went.

SELL opens the seller's desk: what is sold (an agent with its character,
skills and, unless unticked, its notes; a hamlet with all its agents; or
tools of an agent's house), its words, its category, tags, price and an
optional cover. The server reads the package and shows buyers what is
really in it, the permissions a plugin's manifests ask for among it. A
paid listing needs payouts set up with Stripe first. YOURS lists what the
chief sells, to edit, upload a new version of, put on or take off the
shelves, and what they bought, to install or update.
## Starter skills

`crates/hamlet_home/starters/` holds the starter skills: web search, page
reading, weather, API calls, system info, and guidance on writing skills
and notes. They are copied into a workspace the first time it is opened and
belong to the agent after that.
