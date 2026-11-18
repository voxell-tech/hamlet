/**
 * The parts of `@earendil-works/pi-coding-agent` the host uses. The
 * package's root also loads its interactive mode and terminal UI, which
 * the host never shows, and its exports name no smaller entry, so each
 * part is loaded from its file beside the root.
 */
import type * as CodingAgent from "@earendil-works/pi-coding-agent";

export type { CreateModelRuntimeOptions } from "@earendil-works/pi-coding-agent";

const root = import.meta.resolve("@earendil-works/pi-coding-agent");

/**
 * The `names` the file exports, under the root's types. A release that
 * moves one fails here, at load.
 */
async function part<N extends keyof typeof CodingAgent & string>(
    file: string,
    ...names: N[]
): Promise<Pick<typeof CodingAgent, N>> {
    const loaded: Record<string, unknown> = await import(
        new URL(file, root).href
    );
    const missing = names.filter((name) => loaded[name] === undefined);
    if (missing.length > 0) {
        throw new Error(
            `@earendil-works/pi-coding-agent's ${file} exports no ${missing.join(", ")}`,
        );
    }
    return loaded as Pick<typeof CodingAgent, N>;
}

export const { ModelRuntime } = await part(
    "./core/model-runtime.js",
    "ModelRuntime",
);
export type ModelRuntime = CodingAgent.ModelRuntime;

export const { generateDiffString, generateUnifiedPatch } = await part(
    "./core/tools/edit-diff.js",
    "generateDiffString",
    "generateUnifiedPatch",
);
