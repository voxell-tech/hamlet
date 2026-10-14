// Writes protocol v0 as JSON Schema to protocol.schema.json.
import { writeFileSync } from "node:fs";
import { protocolSchema } from "../src/protocol.ts";

writeFileSync(
    new URL("../protocol.schema.json", import.meta.url),
    `${JSON.stringify(protocolSchema(), null, 4)}\n`,
);
