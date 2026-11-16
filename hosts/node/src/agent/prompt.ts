/**
 * The system prompt, one pi-durable section per part (C11), rendered before
 * every model request, so a change shows from the next call.
 */
import { defineExtension, type Extension } from "@earendil-works/pi-durable";
import type { ReadCopies } from "./copies.ts";
import type { Peers } from "./turn.ts";

/** What the prompt's parts are read from, by the sections and by `read_prompt`. */
export interface PromptSources {
    readonly copies: ReadCopies;
    readonly peers: Peers;
    /** An errand's prompt is its own instructions only. */
    readonly errand?: string;
}

export function promptPart(sources: PromptSources): Extension {
    void sources;
    return defineExtension({ name: "hamlet.prompt" });
}

/** The whole prompt as the next request would see it. */
export function renderPrompt(sources: PromptSources): Promise<string> {
    void sources;
    return Promise.reject(new Error("not built: U1"));
}
