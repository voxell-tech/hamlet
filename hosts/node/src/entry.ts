/**
 * What every entry of the Node host does first and last: it takes the link
 * from the Rust host that started it, and serves its kinds until that host's
 * goodbye. A failure is named on standard error under the entry's name, and
 * the process exits 1.
 */

// Only Node's own modules and the native module are imported before the
// take. The actor layer takes longer to load, and on macOS nothing ends this
// process with its Rust host until the take.
import { takeLink } from "@hamlet/actor-node";
import type { AnyActorDefinition } from "./actor/actor/definition.ts";

const NO_LINK =
    "this process has no link to a Rust host. A Rust host starts the Node host itself, with nothing between them: Node was started by hand or through a launcher (a version manager's shim, a shell script, npx). Set HAMLET_NODE to the Node executable itself.";

export function fail(entry: string, words: string): never {
    console.error(`${entry}: ${words}`);
    process.exit(1);
}

export function take(entry: string): void {
    try {
        takeLink();
    } catch {
        fail(entry, NO_LINK);
    }
}

/** Serves `kinds` over the link until the goodbye, then exits 0. */
export async function serve(
    entry: string,
    kinds: Readonly<Record<string, AnyActorDefinition>>,
): Promise<never> {
    const { serveLink } = await import("./actor/registry/link.ts");
    try {
        await serveLink(kinds);
    } catch (error) {
        fail(entry, reason(error));
    }
    process.exit(0);
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
