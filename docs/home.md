# The home

Everything Hamlet keeps is in one folder, `~/.hamlet`. `HAMLET_HOME` moves
it elsewhere.

```
~/.hamlet/
  config.toml     settings and endpoints (keys are in the keychain, or in api_key.<endpoint>)
  models.toml     the models you added, with what each costs
  bazaar.json     what OpenRouter and the endpoints offer, asked again each day; safe to delete
  skills_sh.json  the skills skills.sh lists, asked again each day; safe to delete
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
and the Inspector becomes its keeper's counter, the same wares as that
stall's tab in the bazaar's panel. Where a chief set it down is kept in
`bazaar.place`.

| Stall | Keeper | Sells | Buying |
|---|---|---|---|
| Agent Stall | Brannoc, the mind-monger | Models, as below | Adds the model to `models.toml` |
| Artisans' Stall | Wren Halloway | Agents other chiefs made (mock) | Pretend: kept only while the app runs |
| Tinker's Stall | Tobias Cogsworth | Plugins that add to Hamlet (mock) | Pretend: kept only while the app runs |
| Skill Stall | Sister Marjoram | Skills listed on skills.sh | Shelves the skill in `library/`, recorded in `library.jsonl` with `from` its source; no librarian is asked, since the chief chose it |
| Hamlet Store | Lady Ashgrove | Whole hamlets (mock), each shown as a turning diorama | Founds the hamlet at once: its mayor, its agents and their characters, notes and skills |

Adding a model writes it to `models.toml`, allowed; one from OpenRouter
first signs the chief in to OpenRouter when the home has no endpoint
there. Only added models are in `models.toml`: List Models updates
their prices and drops those an endpoint no longer serves.

## Starter skills

`crates/hamlet_home/starters/` holds the starter skills: web search, page
reading, weather, API calls, system info, and guidance on writing skills
and notes. They are copied into a workspace the first time it is opened and
belong to the agent after that.
