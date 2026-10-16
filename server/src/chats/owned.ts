import type { ConversationId, Tx } from "@earendil-works/pi-durable";
import { liveHamlets, RealmDoc } from "../realm/places.ts";
import { type AgentRef, agentKey, RecordDoc } from "../record.ts";
import { AgentConversationsDoc, type With, WithDoc } from "./conversations.ts";

/** Living agents' indexed conversations, read in one commit. */
export async function agentConversations(
    tx: Tx,
): Promise<{ id: ConversationId; place: With }[]> {
    const result: { id: ConversationId; place: With }[] = [];
    for (const { id } of liveHamlets(await tx.doc(RealmDoc))) {
        for (const agent of (await tx.doc(RecordDoc, id, id)).agents) {
            result.push(
                ...(await conversationsFor(tx, {
                    hamlet: id,
                    number: agent.number,
                })),
            );
        }
    }
    return result;
}

/** All indexed conversations of an agent, including older sessions. */
export async function conversationsFor(
    tx: Tx,
    agent: AgentRef,
): Promise<{ id: ConversationId; place: With }[]> {
    const key = agentKey(agent);
    const result: { id: ConversationId; place: With }[] = [];
    for (const id of (await tx.doc(AgentConversationsDoc, key, key)).ids)
        result.push({ id, place: await tx.doc(WithDoc, id) });
    return result;
}
