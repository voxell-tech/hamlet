/**
 * The agent's model access: the model its copy names, the key the realm
 * gives for it, held in memory only, and the price each call is charged at.
 */
import type { Models } from "@earendil-works/pi-ai";
import type { AgentContext } from "./actor.ts";
import type { ReadCopies } from "./copies.ts";
import type { Peers } from "./turn.ts";

export function agentModels(
    listed: Models,
    copies: ReadCopies,
    peers: Peers,
): Models {
    void copies;
    void peers;
    return listed;
}

/** Moves every conversation of the agent to the model its copy names. */
export async function configured(
    c: AgentContext,
    copies: ReadCopies,
): Promise<void> {
    void c;
    void copies;
}
