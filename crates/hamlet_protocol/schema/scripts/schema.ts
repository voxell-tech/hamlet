// Writes protocol v0 as JSON Schema beside the Rust crate.
import { writeFileSync } from "node:fs";
import { protocolSchema } from "../src/protocol.ts";

writeFileSync(
    new URL("../../schema.json", import.meta.url),
    `${JSON.stringify(protocolSchema(), null, 4)}\n`,
);
