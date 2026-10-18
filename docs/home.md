# The home

Everything Hamlet keeps is in one folder, `~/.hamlet`. `HAMLET_HOME` moves
it elsewhere.

```
~/.hamlet/
  config.toml     settings and endpoints (keys are in the keychain, or in api_key.<endpoint>)
  models.toml     the models you allow, with what each costs
  library/        the skills every agent shares, one folder each
  library.jsonl   what was shelved there, refused and taken out
  charter.md      your own charter, the words every agent's prompt opens with; absent until you edit it
  CHARACTER.md    the mayor's character, if you gave it one; no tool of its own can write it
  workspace/      the only place the mayor's tools can write, besides a folder its hamlet works on
    NOTES.md      its memory, shown to it every turn
    skills/       its skills, one folder each with a SKILL.md
  plugins/        installed WASM plugins, one folder each, and approved.toml (see wasm.md)
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

## Starter skills

`crates/hamlet_home/starters/` holds the starter skills: web search, page
reading, weather, API calls, system info, and guidance on writing skills
and notes. They are copied into a workspace the first time it is opened and
belong to the agent after that.
