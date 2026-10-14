import type { Context } from "@earendil-works/chord";
import type { Extension } from "@earendil-works/pi-durable";
import { Budget } from "./budget.ts";
import { Choice } from "./choice.ts";
import { Gate } from "./gate.ts";
import { Hand } from "./hand.ts";
import type { Home } from "./home.ts";
import { Providers } from "./models.ts";
import { Prompt } from "./prompt.ts";
import { Talk } from "./talk.ts";
import { Tools } from "./tools.ts";

/**
 * One concept's share of an open home: an extension, installed in the order of the parts, and work it starts once
 * every part is installed and stops before the home closes.
 */
export type Part = (home: Home) => {
    readonly extension?: Extension;
    start?(context: Context): Promise<void>;
    stop?(context: Context): Promise<void>;
};

/**
 * The home's own parts, in the order they install. The tools added by hand come before Hamlet's, so that Hamlet's own
 * win any name.
 */
export const PARTS: readonly Part[] = [
    Prompt,
    Hand,
    Tools,
    Gate,
    Talk,
    Budget,
    Choice,
    Providers,
];
