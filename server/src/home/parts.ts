import { Budget } from "../budget.ts";
import { Choice } from "../choice.ts";
import { Gate } from "../gate.ts";
import { Hand } from "../hand.ts";
import type { Part } from "../home.ts";
import { Librarian } from "../library.ts";
import { Providers } from "../models.ts";
import { Objections } from "../oversight.ts";
import { Prompt } from "../prompt.ts";
import { Dealings, Founding } from "../realm.ts";
import { Agents } from "../society/agents.ts";
import { HamletBrief } from "../society/brief.ts";
import { Leaving } from "../society/leaving.ts";
import { Letters } from "../society/letters.ts";
import { Merging } from "../society/merging.ts";
import { Votes } from "../society/votes.ts";
import { Talk } from "../talk.ts";
import { Tools } from "../tools.ts";

/**
 * The home's own parts, in the order they install. The tools added by hand come before Hamlet's, so that Hamlet's own
 * win any name.
 */
export function parts(): readonly Part[] {
    return [
        Prompt,
        HamletBrief,
        Founding,
        Hand,
        Tools,
        Agents,
        Letters,
        Leaving,
        Merging,
        Votes,
        Librarian,
        Objections,
        Dealings,
        Gate,
        Talk,
        Budget,
        Choice,
        Providers,
    ];
}
