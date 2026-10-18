// Writes the harness link as JSON Schema to link.schema.json.
import { writeFileSync } from "node:fs";
import { linkSchema } from "../src/link.ts";

writeFileSync(
    new URL("../link.schema.json", import.meta.url),
    `${JSON.stringify(linkSchema(), null, 4)}\n`,
);
