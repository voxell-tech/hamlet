import type { Context } from "@earendil-works/chord";
import type { Conversation } from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import { PeersDoc } from "../letters.ts";
import type { ChatRef } from "../protocol.ts";
import { livingAgent } from "../realm/roads.ts";
import { type AgentRef, agentKey, RecordDoc } from "../record.ts";
import { err, ok, type Result } from "../result.ts";
import type { WatchFailure } from "./failure.ts";

/** A chief's read-only conversation between two agents. */
export async function agentConversation(
    home: Home,
    chat: ChatRef,
    context: Context,
): Promise<Result<Conversation, WatchFailure>> {
    if (!chat.from)
        return err({
            kind: "no-peer-conversation",
            reason: "choose-peer",
            operation: "open chat",
            chat,
            retrySafe: true,
        });
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
        return err({
            kind: "no-peer-conversation",
            reason: "session-number",
            operation: "open chat",
            chat,
            retrySafe: true,
        });
    const id = peers?.[chat.kind ?? "letters"][key];
    const conversation =
        id === undefined
            ? undefined
            : await home.harness.conversation(id, context);
    if (!conversation)
        return err({
            kind: "no-peer-conversation",
            reason: "missing",
            operation: "open chat",
            chat,
            retrySafe: true,
        });
    return { ok: true, value: conversation };
}

/** Refusal of changes to a watched conversation between agents. */
export function requireWritable(chat: ChatRef): Result<void, WatchFailure> {
    if (chat.from)
        return err({
            kind: "read-only-peer",
            operation: "rewind",
            chat,
            retrySafe: true,
        });
    if (chat.past !== undefined)
        return err({
            kind: "past-read-only",
            operation: "rewind",
            chat,
            retrySafe: true,
        });
    return ok(undefined);
}

/** A peer writer's current name and hamlet, following recorded moves by number. */
export async function peerAuthor(
    home: Home,
    original: AgentRef,
    context: Context,
): Promise<string> {
    let ref = original;
    const seen = new Set<string>();
    while (!seen.has(agentKey(ref))) {
        seen.add(agentKey(ref));
        const record = await home.harness.snapshot(
            RecordDoc,
            ref.hamlet,
            context,
        );
        const agent =
            record?.agents.find((a) => a.number === ref.number) ??
            record?.archive.find((a) => a.agent.number === ref.number)?.agent;
        if (agent) return `${agent.name}@${ref.hamlet}`;
        const moved = record?.decisions.findLast(
            (decision) =>
                decision.kind === "moved_out" && decision.agent === ref.number,
        );
        if (moved?.kind !== "moved_out") break;
        const target = await home.harness.snapshot(
            RecordDoc,
            moved.hamlet,
            context,
        );
        const arrival = target?.decisions.find(
            (decision) =>
                decision.kind === "moved_in" &&
                decision.hamlet === ref.hamlet &&
                decision.name === moved.name &&
                decision.at === moved.at,
        );
        if (arrival?.kind !== "moved_in") break;
        ref = { hamlet: moved.hamlet, number: arrival.agent };
    }
    return `${original.hamlet}#${original.number}`;
}
