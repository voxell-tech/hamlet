import { Agents, Leaving, Merging } from "../agents.ts";
import { Talk } from "../chats.ts";
import { Gate } from "../gate.ts";
import { Hand } from "../hand.ts";
import type { Part } from "../home.ts";
import { Letters } from "../letters.ts";
import { Librarian } from "../library.ts";
import { Choice } from "../models.ts";
import { Objections } from "../oversight.ts";
import { HamletBrief, Prompt } from "../prompt.ts";
import { Dealings, Founding } from "../realm.ts";
import { Budget } from "../spend.ts";
import { Tools } from "../tools.ts";
import { Votes } from "../votes.ts";

/**
 * The home's own parts, in the order they install. The tools added by hand come before Hamlet's, so that Hamlet's own
 * win any name.
 */
export const PARTS: readonly Part[] = [
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
];
