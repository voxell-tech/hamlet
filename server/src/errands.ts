import type { Context } from "@earendil-works/chord";
import {
    AssistantEntry,
    type ConversationId,
    configure,
    type DocumentReader,
    defineDoc,
    type ToolRegistration,
    type Tx,
} from "@earendil-works/pi-durable";
import { ErrandEntry, startConversation, WithDoc } from "./chats.ts";
import type { Home } from "./home.ts";
import { configureAgent } from "./models.ts";
import type { AgentRef } from "./record.ts";

/** The purpose of a one-off model call. */
export type ErrandKind = "ballot" | "answer" | "librarian" | "notes" | "review";

/** The role and prompt ownership of a one-off call. */
export const ErrandDoc = defineDoc<{
    kind: ErrandKind | null;
    own: boolean;
}>({
    kind: "hamlet.errand",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "current",
    initial: () => ({ kind: null, own: true }),
});
/** A conversation's paying hamlet (H14). */
export const PayerDoc = defineDoc<{ hamlet: string | null }>({
    kind: "hamlet.payer",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "current",
    initial: () => ({ hamlet: null }),
});
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
/** The settled status and text of an idempotent errand input. */
export async function runErrand(
    home: Home,
    id: ConversationId,
    given: { requestId: string; text: string; task: string },
    context: Context,
): Promise<{ status: string; text: string }> {
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
    const settled = await submission.wait(context);
    if (settled.type !== "input" || settled.status !== "done")
        return {
            status: settled.status,
            text: "reason" in settled ? settled.reason : "It could not finish.",
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
