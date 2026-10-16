import type { Context } from "@earendil-works/chord";
import type { Conversation } from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import { PeersDoc } from "../letters.ts";
import type { ChatRef } from "../protocol.ts";

import { type AddressFailure, livingAgent } from "../realm/roads.ts";
import { agentKey } from "../record.ts";
import type { Result } from "../result.ts";

/** A chief's read-only conversation between two agents. */
export async function agentConversation(
    home: Home,
    chat: ChatRef,
    context: Context,
): Promise<Result<Conversation, AddressFailure>> {
    if (!chat.from) throw new Error("Choose the other agent's conversation.");
    const target = await livingAgent(
        home,
        chat.with,
        "watch conversations",
        context,
    );
    if (!target.ok) return target;
    const peers = await home.harness.snapshot(
        PeersDoc,
        agentKey(target.value.ref),
        context,
    );
    const key = agentKey(chat.from);
    if (chat.past !== undefined)
        throw new Error("A peer conversation has no chief-session number.");
    const id = peers?.[chat.kind ?? "letters"][key];
    const conversation =
        id === undefined
            ? undefined
            : await home.harness.conversation(id, context);
    if (!conversation)
        throw new Error("There is no conversation with that agent here.");
    return { ok: true, value: conversation };
}

/** Refusal of changes to a watched conversation between agents. */
export function requireWritable(chat: ChatRef): void {
    if (chat.from)
        throw new Error("Conversations between agents are read-only.");
}
