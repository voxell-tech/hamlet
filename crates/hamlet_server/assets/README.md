# Protocol validation inputs

`limits.schema.json` defines the chief's limits, including defaults and
constraints. `src/limits.rs` uses it for validation and filling defaults.
Edit it here when changing a limit; keep the client protocol's `Limits`
in `crates/hamlet_protocol/src/client/limits.rs` consistent.

`provider-names.json` holds the reserved built-in endpoint names. Regenerate
it from the repository root after `npm ci --prefix harnesses/pi`, using
the Node version in `harnesses/pi/.nvmrc`:

```sh
node --input-type=module -e 'import { builtinProviders } from "./harnesses/pi/node_modules/@earendil-works/pi-ai/dist/providers/all.js"; console.log(JSON.stringify([...builtinProviders().map(p => p.id), "chatgpt"], null, 2))' > crates/hamlet_server/assets/provider-names.json
```

`protocol.rs` derives the client protocol's JSON Schema from the Rust
types in `hamlet_protocol` and validates each client message against it
before decoding.

`charter.md` and `starters/` are embedded in the server binary. The starter
installer in `src/starters.rs` preserves chiefs' edits and deletions.
