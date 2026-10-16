import { randomUUID } from "node:crypto";
import type { Context } from "@earendil-works/chord";
import {
    type ConversationId,
    type DocumentReader,
    type Harness,
    LiveDoc,
    type SubmissionId,
} from "@earendil-works/pi-durable";
import type { AgentRef } from "../record.ts";
import {
    ChiefEntry,
    ErrandEntry,
    LetterEntry,
    QuestionEntry,
    ReflectionEntry,
    ReplyEntry,
    RetryEntry,
} from "./conversations.ts";

/** A turn: the run one input starts, until its answer. */
export type Turn = {
    /** The input that started it. */
    input: SubmissionId;
    /** The kind of the input's entry, such as `hamlet.chief`. */
    kind: string;
    /** The H12 task the input carries; `null` for an input that carries none. */
    task: string | null;
    /** The chief whose typed line started it. */
    chief: string | null;
    from: AgentRef | null;
};

/**
 * The turn a conversation runs now, `undefined` while it runs none. It is the same through every generation of a run,
 * which `LiveDoc.run.taskId` is not.
 */
export async function turnOf(
    harness: Harness,
    api: DocumentReader & { readonly conversationId: ConversationId },
    context: Context,
): Promise<Turn | undefined> {
    const live = await api.snapshot(LiveDoc, api.conversationId, context);
    const input = live?.run?.inputs[0];
    if (input === undefined) return undefined;
    const submission = await harness.submission(input, context);
    const placed = (await submission?.status(context))?.entry;
    const conversation = await harness.conversation(
        api.conversationId,
        context,
    );
    const entry =
        placed === undefined
            ? undefined
            : (
                  await conversation?.entries(
                      { minEntryId: placed, maxEntryId: placed },
                      1,
                      undefined,
                      context,
                  )
              )?.items[0];
    if (ChiefEntry.is(entry)) {
        const { task, chief } = entry.data;
        return { input, kind: entry.kind, task, chief, from: null };
    }
    if (
        LetterEntry.is(entry) ||
        QuestionEntry.is(entry) ||
        ReplyEntry.is(entry)
    )
        return {
            input,
            kind: entry.kind,
            task: entry.data.task,
            chief: null,
            from: entry.data.from,
        };
    if (
        RetryEntry.is(entry) ||
        ReflectionEntry.is(entry) ||
        ErrandEntry.is(entry)
    ) {
        return {
            input,
            kind: entry.kind,
            task: entry.data.task,
            chief: null,
            from: null,
        };
    }
    return {
        input,
        kind: entry?.kind ?? "pi.user",
        task: null,
        chief: null,
        from: null,
    };
}

/** A new H12 task, begun at `at`. */
export function newTask(at: number): string {
    return `${at}-${randomUUID()}`;
}
