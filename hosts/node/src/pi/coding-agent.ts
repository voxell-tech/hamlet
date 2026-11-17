/**
 * The parts of `@earendil-works/pi-coding-agent` the host uses. The
 * package's root also loads its interactive mode and terminal UI, which
 * the host never shows, and its exports name no smaller entry, so each
 * part is loaded from its file beside the root.
 */
import type * as CodingAgent from "@earendil-works/pi-coding-agent";

export type { CreateModelRuntimeOptions } from "@earendil-works/pi-coding-agent";

const root = import.meta.resolve("@earendil-works/pi-coding-agent");

async function part<T>(file: string): Promise<T> {
    return (await import(new URL(file, root).href)) as T;
}

export const { ModelRuntime } = await part<
    Pick<typeof CodingAgent, "ModelRuntime">
>("./core/model-runtime.js");
export type ModelRuntime = CodingAgent.ModelRuntime;

export const { generateDiffString, generateUnifiedPatch } = await part<
    Pick<typeof CodingAgent, "generateDiffString" | "generateUnifiedPatch">
>("./core/tools/edit-diff.js");
