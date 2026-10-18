# Protocol validation inputs

`limits.schema.json` is the TypeScript server's `LimitsSchema`, including
its defaults. `provider-names.json` holds the reserved built-in endpoint
names. Regenerate them from the repository root after `npm ci --prefix
server`, using the Node version in `server/.nvmrc`:

```sh
node --input-type=module -e 'import { LimitsSchema } from "./server/src/limits/rules.ts"; console.log(JSON.stringify(LimitsSchema, null, 2))' > crates/hamlet-server/assets/limits.schema.json
node --input-type=module -e 'import { builtinProviders } from "./server/node_modules/@earendil-works/pi-ai/dist/providers/all.js"; console.log(JSON.stringify([...builtinProviders().map(p => p.id), "chatgpt"], null, 2))' > crates/hamlet-server/assets/provider-names.json
```

The wire-message schema stays in `server/protocol.schema.json`, shared
with `hamlet_protocol_gen`. `protocol.rs` validates against it before
decoding the generated Rust types.
