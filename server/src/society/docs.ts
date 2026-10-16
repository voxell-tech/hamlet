import {
    type ConversationId,
    defineDocFamily,
} from "@earendil-works/pi-durable";
import type { VoteView } from "../protocol.ts";

/** A stored vote, including each missed or grounded ballot (H13). */
export type Vote = {
    id: number;
    motion:
        | { kind: "replace_mayor"; candidate: number }
        | {
              kind: "found_hamlet";
              hamlet: string;
              mayor: string;
              purpose: string;
          };
    by: { agent: number } | { chief: string };
    reason: string;
    mayor: number;
    voters: number[];
    answer: string | null;
    ballots: {
        voter: number;
        choice: "yes" | "no" | "abstain" | null;
        reason: string;
        evidence: string;
        grounded: boolean;
    }[];
    standing: "open" | "passed" | "overturned" | { failed: string };
    called: number;
    task: string;
};
/** Each hamlet's votes. */
export const VotesDoc = defineDocFamily<{ votes: Vote[] }, string>({
    kind: "hamlet.votes",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ votes: [] }),
});
/** Each agent's incoming conversations, by sender key (C22). */
export const PeersDoc = defineDocFamily<
    {
        letters: { [key: string]: ConversationId };
        questions: { [key: string]: ConversationId };
    },
    string
>({
    kind: "hamlet.peers",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ letters: {}, questions: {} }),
});
/** Every conversation belonging to an agent. */
export const AgentConversationsDoc = defineDocFamily<
    { ids: ConversationId[] },
    string
>({
    kind: "hamlet.agent-conversations",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ ids: [] }),
});
/** A vote's public words. */
export type VoteWords = VoteView;
