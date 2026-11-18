/**
 * The Node host's entry: `node src/main.ts --kind <kind>...`, started by a
 * Rust host as its bound child. It takes the link, serves the kinds of
 * `demo.ts` it is given until the Rust host's goodbye, and exits 0, or 1
 * when it could not.
 */

import { parseArgs } from "node:util";
import { fail, serve, take } from "./entry.ts";

const ENTRY = "hamlet node host";

take(ENTRY);

const { kinds } = await import("./demo.ts");

/** The kinds of `demo.ts` the `--kind` arguments name. */
function served(): Record<string, (typeof kinds)[keyof typeof kinds]> {
    let named: string[];
    try {
        named =
            parseArgs({ options: { kind: { type: "string", multiple: true } } })
                .values.kind ?? [];
    } catch (error) {
        fail(ENTRY, String(error));
    }
    if (named.length === 0) fail(ENTRY, "name the kinds to serve with --kind");
    return Object.fromEntries(
        named.map((kind) => {
            if (!Object.hasOwn(kinds, kind)) {
                fail(ENTRY, `demo.ts has no kind '${kind}'`);
            }
            return [kind, kinds[kind as keyof typeof kinds]];
        }),
    );
}

await serve(ENTRY, served());
