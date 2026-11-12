# Server assets

`limits.schema.json` is a JSON Schema of the chief's limits. The server
does not read it: the limits are the Rust types in
`crates/hamlet_protocol/src/client/limits.rs`.

`provider-names.json` holds the reserved built-in endpoint names. Regenerate
it from the repository root after `npm ci --prefix harnesses/pi`, using
the Node version in `harnesses/pi/.nvmrc`:

```sh
node --input-type=module -e 'import { builtinProviders } from "./harnesses/pi/node_modules/@earendil-works/pi-ai/dist/providers/all.js"; console.log(JSON.stringify([...builtinProviders().map(p => p.id), "chatgpt"], null, 2))' > crates/hamlet_server/assets/provider-names.json
```

`charter.md`, `hand.md` and `starters/` are embedded in the server
binary. `src/knowledge/library.rs` commits the starters to a new
library's `main`.

The client protocol's JSON Schema is derived from the Rust types in
`hamlet_protocol` by `src/clients/check.rs`, not read from here.
