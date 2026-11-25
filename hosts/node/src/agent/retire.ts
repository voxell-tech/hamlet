/** The realm's calls of an ended hire's agent: `retire` and `erase`. */
import { type AgentCall, generationOf } from "./actor.ts";
import { stop } from "./commands.ts";
import type { Retired } from "./messages/agent.ts";

/** Stops the agent's turns as `stop` does, then saves its filesystem to its branch. */
export async function retire(c: AgentCall): Promise<Retired> {
    await stop(c);
    const tip = await generationOf(c).branch.save();
    if (tip === undefined) {
        throw new Error(
            "the agent was filled from no profile: it has no branch",
        );
    }
    return { tip };
}

/**
 * Removes the agent's filesystem, then asks the host to destroy the agent,
 * which removes its database once this has answered.
 */
export async function erase(c: AgentCall): Promise<void> {
    await generationOf(c).branch.erase();
    c.destroy();
}
