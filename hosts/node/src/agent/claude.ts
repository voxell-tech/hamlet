/**
 * The provider `claude`: turns run by Anthropic's own program through its
 * Agent SDK, in the realm's `claude/` folder and an environment without
 * `ANTHROPIC_*`, `CLAUDE*` or `HAMLET_*`.
 */
import type { Models } from "@earendil-works/pi-ai";

export interface ClaudeProgram {
    /** The program to run; tests name a fake that answers on loopback. */
    readonly executable: string;
    readonly folder: string;
}

export function withClaude(models: Models, program: ClaudeProgram): Models {
    void program;
    return models;
}
