import {
    type ConversationId,
    defineEntry,
    type Tx,
} from "@earendil-works/pi-durable";

/** A transcript entry meant only for people, which the model never sees (C30). */
export const NoticeEntry = defineEntry<{ text: string; at: number }>(
    "hamlet.notice",
);

/** Appends a notice to `conversation`, made at `at`. */
export async function notify(
    tx: Tx,
    conversation: ConversationId,
    text: string,
    at: number,
): Promise<void> {
    await tx.appendEntry(NoticeEntry, conversation, { data: { text, at } });
}
