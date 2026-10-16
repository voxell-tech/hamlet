import type { Context } from "@earendil-works/chord";
import type { Conversation } from "@earendil-works/pi-durable";
import type { Home } from "./home.ts";
import { PeersDoc } from "./letters.ts";
import type { ChatRef } from "./protocol.ts";
import { type ChiefResult, chiefAccess, resolve } from "./realm.ts";
import { agentKey } from "./record.ts";

/** A chief's read-only conversation between two agents. */
export async function agentConversation(
    home: Home,
    chat: ChatRef,
    context: Context,
): Promise<ChiefResult<Conversation>> {
    if (!chat.from) throw new Error("Choose the other agent's conversation.");
    const access = await chiefAccess(
        home,
        chat.with.hamlet,
        "watch conversations",
        context,
    );
    if (!access.ok) return access;
    const target = await resolve(
        home.harness,
        chat.with.hamlet,
        `${chat.with.agent}@${chat.with.hamlet}`,
        context,
    );
    if (!target) throw new Error(`no agent named \`${chat.with.agent}\``);
    if ("gone" in target) throw new Error(target.gone);
    const peers = await home.harness.snapshot(
        PeersDoc,
        agentKey(target.ref),
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
