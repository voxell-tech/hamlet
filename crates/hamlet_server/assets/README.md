# Protocol validation inputs

`limits.schema.json` defines the chief's limits, including defaults and
constraints. `src/limits.rs` uses it for validation and filling defaults.
Edit it here when changing a limit; keep protocol v0's limits shape in
`crates/hamlet_protocol/schema/` consistent.

`provider-names.json` holds the reserved built-in endpoint names. Regenerate
it from the repository root after `npm ci --prefix harnesses/pi`, using
the Node version in `harnesses/pi/.nvmrc`:

```sh
node --input-type=module -e 'import { builtinProviders } from "./harnesses/pi/node_modules/@earendil-works/pi-ai/dist/providers/all.js"; console.log(JSON.stringify([...builtinProviders().map(p => p.id), "chatgpt"], null, 2))' > crates/hamlet_server/assets/provider-names.json
```

The wire-message schema lives in `crates/hamlet_protocol/schema.json`, shared
with `hamlet_protocol_gen`. `protocol.rs` validates against it before
decoding the generated Rust types.

`charter.md` and `starters/` are embedded in the server binary. The starter
installer in `src/starters.rs` preserves chiefs' edits and deletions.
