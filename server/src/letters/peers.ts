import type { Context } from "@earendil-works/chord";
import {
    type ConversationId,
    defineDocFamily,
} from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import type { Agent } from "../protocol.ts";
import { type AgentRef, agentKey } from "../record.ts";
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

type PeerView = NonNullable<Agent["conversations"]>[number];

/** The incoming peer conversations of an agent (C22). */
export async function conversationViews(
    home: Home,
    agent: AgentRef,
    context: Context,
): Promise<PeerView[]> {
    const peers = await home.harness.snapshot(
        PeersDoc,
        agentKey(agent),
        context,
    );
    const views: PeerView[] = [];
    for (const kind of ["letters", "questions"] as const)
        for (const [key, id] of Object.entries(peers?.[kind] ?? {})) {
            const split = key.lastIndexOf(":");
            const hamlet = key.slice(0, split),
                number = Number(key.slice(split + 1));
            if (split < 1 || !Number.isSafeInteger(number) || number < 0)
                continue;
            const stats = await home.peerStatistics.read(id, context);
            if (stats) views.push({ with: { hamlet, number }, kind, ...stats });
        }
    return views.sort((a, b) => (b.last ?? 0) - (a.last ?? 0));
}
