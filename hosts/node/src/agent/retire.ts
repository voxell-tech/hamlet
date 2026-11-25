/** The realm's calls of an ended hire's agent: `retire` and `erase`. */
import { type AgentCall, generationOf } from "./actor.ts";
import { stop } from "./commands.ts";
import type { Retired } from "./messages/agent.ts";

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

export async function erase(c: AgentCall): Promise<void> {
    await generationOf(c).branch.erase();
    // The host removes the database once this has answered.
    c.destroy();
}
