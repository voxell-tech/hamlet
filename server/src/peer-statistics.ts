import type { Context } from "@earendil-works/chord";
import type {
    ConversationId,
    Cursor,
    EntryRecord,
    Harness,
} from "@earendil-works/pi-durable";
import { LetterEntry, QuestionEntry, ReplyEntry } from "./chats.ts";

type Statistics = { count: number; last: number | null };

/** Peer history totals, scanned once per open home and advanced by committed entries. */
export class PeerStatistics {
    readonly #harness: Harness;
    readonly #values = new Map<ConversationId, Statistics>();
    readonly #unsubscribe: () => void;

    /** Owns a commit subscription until close; entries are append-only. */
    constructor(harness: Harness) {
        this.#harness = harness;
        this.#unsubscribe = harness.subscribeCommits(({ changes }) => {
            for (const change of changes) {
                if (change.type !== "entry") continue;
                const value = this.#values.get(change.value.conversationId);
                if (value) advance(value, change.value);
            }
        });
    }

    /** Loads an uncached history on the mutation line so no committed entry can be missed. */
    async read(
        id: ConversationId,
        context: Context,
    ): Promise<Readonly<Statistics> | undefined> {
        const cached = this.#values.get(id);
        if (cached) return { ...cached };
        return this.#harness.commit(async (tx) => {
            const known = this.#values.get(id);
            if (known) return { ...known };
            if (!(await tx.conversation(id))) return undefined;
            const value: Statistics = { count: 0, last: null };
            let cursor: Cursor | undefined;
            do {
                const page = await tx.scanEntries(
                    { conversationId: id, order: "ascending" },
                    500,
                    cursor,
                );
                for (const entry of page.items) advance(value, entry);
                cursor = page.next;
            } while (cursor !== undefined);
            this.#values.set(id, value);
            return { ...value };
        }, context);
    }

    /** Releases the subscription and the cached totals. */
    close(): void {
        this.#unsubscribe();
        this.#values.clear();
    }
}

function advance(value: Statistics, entry: EntryRecord): void {
    if (
        LetterEntry.is(entry) ||
        QuestionEntry.is(entry) ||
        ReplyEntry.is(entry)
    ) {
        value.count++;
        value.last = Math.max(value.last ?? 0, entry.data.at);
    }
    for (const message of entry.model ?? [])
        if ("timestamp" in message && typeof message.timestamp === "number")
            value.last = Math.max(value.last ?? 0, message.timestamp);
}
