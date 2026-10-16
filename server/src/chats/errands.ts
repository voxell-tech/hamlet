import type { Context } from "@earendil-works/chord";
import {
    AssistantEntry,
    type ConversationId,
    configure,
    type DocumentReader,
    type ToolRegistration,
    type Tx,
} from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import type { AgentRef } from "../record.ts";
import { configureAgent } from "./configure.ts";
import { ErrandEntry, startConversation, WithDoc } from "./conversations.ts";

import { ErrandDoc, type ErrandKind, PayerDoc } from "./documents.ts";

export { ErrandDoc, type ErrandKind, PayerDoc } from "./documents.ts";
/** The agent whose prompt belongs in a conversation. */
export async function agentOf(
    read: DocumentReader,
    conversation: ConversationId,
    context: Context,
) {
    const errand = await read.snapshot(ErrandDoc, conversation, context);
    return errand?.own === false
        ? undefined
        : read.snapshot(WithDoc, conversation, context);
}
/** The hamlet charged for a conversation. */
export async function payerOf(
    read: DocumentReader,
    conversation: ConversationId,
    context: Context,
): Promise<string | undefined> {
    return (
        (await read.snapshot(PayerDoc, conversation, context))?.hamlet ??
        (await read.snapshot(WithDoc, conversation, context))?.hamlet
    );
}
/** A durable one-off conversation, charged to its agent unless assigned a payer. */
export async function startErrand(
    home: Home,
    tx: Tx,
    given: {
        agent: AgentRef;
        kind: ErrandKind;
        own: boolean;
        instructions: string;
        tools: readonly ToolRegistration[];
    },
): Promise<ConversationId> {
    const id = await startConversation(home, tx, {
        ...given.agent,
        chief: null,
        chat: 0,
    });
    Object.assign(await tx.doc(ErrandDoc, id), {
        kind: given.kind,
        own: given.own,
    });
    await configureAgent(home, tx, id);
    await configure(tx, id, {
        instructions: given.instructions,
        tools:
            given.kind === "ballot"
                ? given.tools.filter((t) => t.name === "cast_ballot")
                : [],
    });
    return id;
}
/** The kept answer or classified reason an errand did not finish. */
export type ErrandOutcome =
    | { status: "done"; text: string }
    | { status: "failed" | "aborted" | "timeout"; reason: string };

/** The settled answer of an idempotent errand input; a timeout aborts its conversation. */
export async function runErrand(
    home: Home,
    id: ConversationId,
    given: {
        requestId: string;
        text: string;
        task: string;
        timeoutMs?: number;
    },
    context: Context,
): Promise<ErrandOutcome> {
    const conversation = await home.harness.conversation(id, context);
    if (!conversation) throw new Error("The errand's conversation is gone.");
    const submission = await conversation.submit(
        {
            type: "input",
            requestId: given.requestId,
            content: given.text,
            entry: {
                kind: ErrandEntry.kind,
                data: { task: given.task, at: home.now() },
            },
        },
        context,
    );
    let timer: ReturnType<typeof setTimeout> | undefined;
    const settled = await (async () => {
        try {
            if (given.timeoutMs === undefined)
                return await submission.wait(context);
            const result = await Promise.race([
                submission.wait(context),
                new Promise<null>((resolve) => {
                    timer = setTimeout(
                        () => resolve(null),
                        Math.max(0, given.timeoutMs ?? 0),
                    );
                }),
            ]);
            if (result !== null) return result;
            await conversation.abort(context);
            return null;
        } finally {
            if (timer !== undefined) clearTimeout(timer);
        }
    })();
    if (settled === null)
        return { status: "timeout", reason: "it did not answer in time" };
    if (settled.type !== "input" || settled.status !== "done")
        return {
            status:
                "reason" in settled && settled.reason === "aborted"
                    ? "aborted"
                    : "failed",
            reason:
                "reason" in settled ? settled.reason : "It could not finish.",
        };
    const entry = await home.harness.commit(
        (tx) => tx.entry(AssistantEntry, settled.answer),
        context,
    );
    const message = entry?.model?.[0];
    return {
        status: settled.status,
        text:
            message?.role === "assistant"
                ? message.content
                      .flatMap((p) => (p.type === "text" ? [p.text] : []))
                      .join("\n")
                : "",
    };
}
