/**
 * The Node host's entry: `node src/main.ts --kind <kind>...`, started by a
 * Rust host as its bound child. It takes the link, serves the kinds of
 * `demo.ts` it is given until the Rust host's goodbye, and exits 0, or 1
 * when it could not.
 */

// Only Node's own modules and the native module are imported before the
// take. The actor layer takes longer to load, and on macOS nothing ends this
// process with its Rust host until the take.
import { parseArgs } from "node:util";
import { takeLink } from "@hamlet/actor-node";

const NO_LINK =
    "this process has no link to a Rust host. A Rust host starts the Node host itself, with nothing between them: Node was started by hand or through a launcher (a version manager's shim, a shell script, npx). Set HAMLET_NODE to the Node executable itself.";

function fail(words: string): never {
    console.error(`hamlet node host: ${words}`);
    process.exit(1);
}

try {
    takeLink();
} catch {
    fail(NO_LINK);
}

const [{ serveLink }, { kinds }] = await Promise.all([
    import("./actor/registry/link.ts"),
    import("./demo.ts"),
]);

/** The kinds of `demo.ts` the `--kind` arguments name. */
function served(): Record<string, (typeof kinds)[keyof typeof kinds]> {
    let named: string[];
    try {
        named =
            parseArgs({ options: { kind: { type: "string", multiple: true } } })
                .values.kind ?? [];
    } catch (error) {
        fail(String(error));
    }
    if (named.length === 0) fail("name the kinds to serve with --kind");
    return Object.fromEntries(
        named.map((kind) => {
            if (!Object.hasOwn(kinds, kind)) {
                fail(`demo.ts has no kind '${kind}'`);
            }
            return [kind, kinds[kind as keyof typeof kinds]];
        }),
    );
}

/** `error`'s words, with the cause an uncoded failure keeps in its metadata. */
function reason(error: unknown): string {
    if (!(error instanceof Error)) return String(error);
    const metadata: unknown = "metadata" in error ? error.metadata : undefined;
    const cause =
        typeof metadata === "object" && metadata !== null && "error" in metadata
            ? `: ${String(metadata.error)}`
            : "";
    return `${error.message}${cause}`;
}

try {
    await serveLink(served());
} catch (error) {
    fail(reason(error));
}
process.exit(0);
