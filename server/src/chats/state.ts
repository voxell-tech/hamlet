import type { Context } from "@earendil-works/chord";
import {
    type ConversationId,
    InboxDoc,
    LiveDoc,
} from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import { liveHamlets, RealmDoc } from "../realm/places.ts";
import { type AgentRef, agentKey, RecordDoc } from "../record.ts";
import { activityOf, type Work } from "../state/activity.ts";
import { currentOf } from "./conversations.ts";
import { AgentConversationsDoc, ChatDoc } from "./documents.ts";
import { InstructionsDoc } from "./instructions.ts";

/** Documents that change a chief's view of an agent's conversations. */
export const chatStateDocs = [
    ChatDoc.definition,
    AgentConversationsDoc.definition,
    InstructionsDoc.definition,
];

/** Indexed conversations of living hamlets, including archived agents' chats. */
export async function conversationIds(
    home: Home,
    context: Context,
): Promise<ConversationId[]> {
    const ids = new Set<ConversationId>();
    for (const { id: hamlet } of liveHamlets(
        await home.harness.snapshot(RealmDoc, context),
    )) {
        const record = await home.harness.snapshot(RecordDoc, hamlet, context);
        const agents = [
            ...(record?.agents ?? []),
            ...(record?.archive
                .filter((entry) => entry.back === null)
                .map((entry) => entry.agent) ?? []),
        ];
        for (const agent of agents) {
            const index = await home.harness.snapshot(
                AgentConversationsDoc,
                agentKey({ hamlet, number: agent.number }),
                context,
            );
            for (const id of index?.ids ?? []) ids.add(id);
        }
    }
    return [...ids];
}

/** An agent's selected chief sessions and folded work, without starting a conversation. */
export async function chatState(
    home: Home,
    agent: AgentRef,
    folded: ReadonlyMap<ConversationId, Work>,
    context: Context,
) {
    const chats = await home.harness.snapshot(
        ChatDoc,
        agentKey(agent),
        context,
    );
    const currents = Object.fromEntries(
        Object.entries(chats?.chats ?? {}).map(([chief, chat]) => [
            chief,
            chat.current,
        ]),
    );
    const works: Work[] = [];
    const index = await home.harness.snapshot(
        AgentConversationsDoc,
        agentKey(agent),
        context,
    );
    for (const id of index?.ids ?? []) {
        const observed = folded.get(id);
        if (observed) {
            works.push(observed);
            continue;
        }
        const live = await home.harness.snapshot(LiveDoc, id, context);
        const inbox = await home.harness.snapshot(InboxDoc, id, context);
        works.push({
            working: live?.run !== undefined,
            pending:
                inbox?.items.filter((item) => item.mode !== "write").length ??
                0,
            faulted: false,
            consulting: false,
            activity: activityOf(live?.run === undefined ? "rest" : "think"),
            saying: { text: "", thought: false },
        });
    }
    const current = chats?.chats[home.chief];
    return {
        currents,
        active:
            works.find((work) => work.working) ??
            (current && folded.get(currentOf(current))) ??
            works.at(-1) ??
            null,
        busy: works.some((work) => work.working || work.pending > 0),
        faulted: current
            ? (folded.get(currentOf(current))?.faulted ?? false)
            : false,
        consulting: works.some((work) => work.consulting),
    };
}
