import type { Context } from "@earendil-works/chord";
import type { Conversation } from "@earendil-works/pi-durable";
import type { Home } from "./home.ts";
import type { Agent, ChatRef } from "./protocol.ts";
import { type ChiefResult, chiefAccess, resolve } from "./realm.ts";
import { type AgentRef, agentKey } from "./record.ts";
import { PeersDoc } from "./society/docs.ts";

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
