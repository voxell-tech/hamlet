/**
 * Reflection: after every `reflect_every` answered chief turns in a
 * session, the agent is asked to keep what it learned. The run's `afterRun`
 * hook counts them from the conversation's entries and asks for the task
 * `hamlet.reflect`, which admits the reflection once under
 * `reflection:<conversation>:<k>`.
 */
import {
    type ConversationId,
    defineExtension,
    type Extension,
} from "@earendil-works/pi-durable";
import type { ReadCopies } from "./copies.ts";
import type { WithHarness } from "./turn.ts";

/** The chief turns answered since the last reflection, and that reflection's number. */
export type Owed = (
    conversation: ConversationId,
) => Promise<{ readonly answered: number; readonly k: number }>;

export function reflection(
    copies: ReadCopies,
    owed: Owed,
    every: number | undefined,
): Extension {
    void copies;
    void owed;
    void every;
    return defineExtension({ name: "hamlet.reflection" });
}

export function owedOf(withHarness: WithHarness): Owed {
    void withHarness;
    return () => Promise.reject(new Error("not built: U5"));
}

/** `HAMLET_REFLECT_EVERY`; absent, the copy's setting holds. 0 turns it off. */
export function reflectEvery(): number | undefined {
    const every = process.env.HAMLET_REFLECT_EVERY;
    return every === undefined ? undefined : Number(every);
}
