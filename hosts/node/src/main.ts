/**
 * The Node host's entry, started by a Rust host as its bound child. It
 * takes the link and serves the agent on the realm folder `HAMLET_HOME`
 * until the Rust host's goodbye, and exits 0, or 1 when it could not.
 *
 * `--faux <script>` has the agent's model answer from the script, as
 * `agent/faux.ts` reads it, and `--sign-in` lists two providers on loopback
 * to sign in to. `--kind <kind>...` serves those kinds of `demo.ts` in
 * place of the agent.
 */

import { parseArgs } from "node:util";
import { fail, serve, take } from "./entry.ts";

const ENTRY = "hamlet node host";

take(ENTRY);

let options: { kind?: string[]; faux?: string; "sign-in"?: boolean };
try {
    options = parseArgs({
        options: {
            kind: { type: "string", multiple: true },
            faux: { type: "string" },
            "sign-in": { type: "boolean" },
        },
    }).values;
} catch (error) {
    fail(ENTRY, String(error));
}

await serve(
    ENTRY,
    options.kind === undefined ? await agent() : await demoKinds(options.kind),
);

async function agent() {
    const home = process.env.HAMLET_HOME;
    if (home === undefined) fail(ENTRY, "HAMLET_HOME names no realm folder");
    const { hostedAgent } = await import("./agent/host.ts");
    const faux = options.faux;
    return {
        agent: hostedAgent(home, {
            ...(faux === undefined ? {} : { faux }),
            signIn: options["sign-in"] ?? false,
            claude: faux === undefined,
        }),
    };
}

/** The kinds of `demo.ts` that `named` names. */
async function demoKinds(named: readonly string[]) {
    const { kinds } = await import("./demo.ts");
    return Object.fromEntries(
        named.map((kind) => {
            if (!Object.hasOwn(kinds, kind)) {
                fail(ENTRY, `demo.ts has no kind '${kind}'`);
            }
            return [kind, kinds[kind as keyof typeof kinds]];
        }),
    );
}
