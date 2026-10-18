# Permissions

What an installed plugin may do beyond drawing its own panel. The model is
the one phones use: a manifest names the most a plugin may ask for, each ask
with a reason; the chief chooses for each; a call that is not allowed fails.

## The model

- The manifest's `[[asks]]` name a `capability`, a `reason` (required) and
  optionally a `scope`. Anything the manifest does not name is impossible for
  that plugin.
- Each named capability has one choice: **Allow**, **Ask next time** or
  **Don't allow**. A capability with no choice yet is unanswered and behaves
  as Ask next time.
- The verdict a host function checks is `Granted`, a component on the
  plugin's entity rebuilt from `Choices` and `Asks` whenever either is put
  in. A host function reads it on each call, so a change applies to the next
  call.

## Capabilities

`hamlet_sdk::capability::Capability`: `read.agents`, `read.chat`,
`read.activity`, `read.papers`, `chat.send`, `commands.run`, `ui.menu`, `net`,
`fs.pick`, `fs.folder`, `model.call` and `plugin.use`. Only `read.agents` is
checked by a host function so far; the others can be asked for, shown and
chosen.

## Never grantable

`NEVER` in `hamlet_sdk::capability`: `keys` (keys and model credentials),
`petitions.answer`, `home.raw`, `process.run`, `sandbox.off` and
`storage.other` (another plugin's storage). A manifest that asks for one, for a
name that is not a capability, or with no reason is turned down by the scan,
with the reason logged, and no petition is raised.
