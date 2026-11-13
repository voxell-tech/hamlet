import type { ConversationId, SessionEnd } from "@earendil-works/pi-durable";
import type {
    SqliteDatabase,
    SqliteExecutor,
    SqliteValue,
} from "@earendil-works/pi-durable/storage/sqlite";

/**
 * Writes one row a tool buffered, with the handle of the transaction that
 * stores the tool's result.
 */
export type WriteRow<Row> = (tx: SqliteExecutor, row: Row) => Promise<void>;

/** The call a tool's rows belong to, as pi-durable's tool API names it. */
export interface ToolCall {
    readonly conversationId: ConversationId;
    readonly callId: string;
}

/**
 * Hamlet's database layer under pi-durable's `SqliteStorage`.
 *
 * A tool that sends does not write its rows: it buffers them for its call.
 * When pi-durable stores that call's result, the layer writes the rows with
 * the same transaction handle, right after the result's INSERT, so the
 * result and its rows commit together or not at all. An error result
 * carries no rows: the model hears that the call failed, and a row would
 * send what it may send again.
 */
export interface AgentDatabase<Row> {
    /** The database pi-durable's `SqliteStorage` opens. */
    readonly database: SqliteDatabase;
    /** Holds `rows` for the commit that stores the result of `call`. */
    buffer(call: ToolCall, rows: readonly Row[]): void;
    /** How many rows wait for their tool's result. */
    readonly buffered: number;
    /**
     * Drops every buffered row once `session` has ended, however it ended.
     * A batch pi-durable refuses fails its Session, and neither it nor a
     * later batch is stored. A result the Session did not store by its end
     * never will be, and a new Session that runs the call again buffers its
     * rows again.
     */
    follow(session: { readonly closed: Promise<SessionEnd> }): void;
}

/** One tool result in an INSERT into pi-durable's entries table. */
interface StoredResult {
    readonly key: string;
    readonly isError: boolean;
}

const callKey = (conversationId: unknown, callId: unknown) =>
    JSON.stringify([String(conversationId), String(callId)]);

/**
 * Any statement that writes pi-durable's entries table, under its own name
 * or a prefixed one, which the layer then refuses to read.
 */
const WRITES_ENTRIES = /^\s*(INSERT|REPLACE)\b[^(]*\b\w*entries\b/i;

/**
 * The two forms pi-durable 1.5 writes entries in: rows as one JSON array
 * read through `json_each(?)`, and rows over 8 KiB bound column by column.
 */
const ENTRIES_INSERT =
    /^INSERT INTO entries \(([^)]*)\) (?:(SELECT .* FROM json_each\(\?\) AS item WHERE true)|VALUES (?:\(\?(?:, \?)*\)(?:, )?)+)\s*$/s;

/**
 * The tool results an INSERT into the entries table stores. It throws on an
 * INSERT it cannot read: a result stored without its rows would pass
 * unseen.
 */
function storedResults(sql: string, params: SqliteValue[]): StoredResult[] {
    const insert = ENTRIES_INSERT.exec(sql);
    const columns = insert?.[1]?.split(",").map((column) => column.trim());
    const conversation = columns?.indexOf("conversation_id") ?? -1;
    const record = columns?.indexOf("record") ?? -1;
    if (columns === undefined || conversation < 0 || record < 0) {
        throw new Error(`cannot read pi-durable's entries INSERT: ${sql}`);
    }
    let rows: unknown[][];
    if (insert?.[2] !== undefined) {
        const [array] = params;
        if (params.length !== 1 || typeof array !== "string") {
            throw new Error("pi-durable's entries INSERT has no JSON array");
        }
        rows = JSON.parse(array) as unknown[][];
    } else {
        if (params.length % columns.length !== 0) {
            throw new Error("pi-durable's entries INSERT has partial rows");
        }
        rows = [];
        for (let at = 0; at < params.length; at += columns.length) {
            rows.push(params.slice(at, at + columns.length));
        }
    }
    return rows.flatMap((row) => {
        const text = row[record];
        if (typeof text !== "string") {
            throw new Error("an entries row has no record");
        }
        const entry = JSON.parse(text) as {
            kind?: unknown;
            model?: { toolCallId?: unknown; isError?: unknown }[];
        };
        if (entry.kind !== "pi.tool-result") {
            return [];
        }
        return (entry.model ?? []).map((message) => ({
            key: callKey(row[conversation], message.toolCallId),
            isError: message.isError === true,
        }));
    });
}

export function agentDatabase<Row>(
    base: SqliteDatabase,
    write: WriteRow<Row>,
): AgentDatabase<Row> {
    const pending = new Map<string, readonly Row[]>();

    /**
     * The transaction handle pi-durable gets. `settled` collects the calls
     * whose results it stores, which leave the buffer once COMMIT returns.
     */
    const watched = (
        tx: SqliteExecutor,
        settled: string[],
    ): SqliteExecutor => ({
        exec: (sql) => {
            refuseUnread(sql);
            return tx.exec(sql);
        },
        run: async (sql, ...params) => {
            await tx.run(sql, ...params);
            await joinRows(tx, sql, params, settled);
        },
        runCounted: async (sql, ...params) => {
            const count = await tx.runCounted(sql, ...params);
            await joinRows(tx, sql, params, settled);
            return count;
        },
        get: (sql, ...params) => tx.get(sql, ...params),
        all: (sql, ...params) => tx.all(sql, ...params),
    });

    const joinRows = async (
        tx: SqliteExecutor,
        sql: string,
        params: SqliteValue[],
        settled: string[],
    ) => {
        if (!WRITES_ENTRIES.test(sql)) {
            return;
        }
        for (const result of storedResults(sql, params)) {
            const rows = pending.get(result.key);
            if (rows === undefined) {
                continue;
            }
            if (!result.isError) {
                for (const row of rows) {
                    await write(tx, row);
                }
            }
            settled.push(result.key);
        }
    };

    /** An entries write the layer cannot join must not pass unseen. */
    const refuseUnread = (sql: string) => {
        if (WRITES_ENTRIES.test(sql)) {
            throw new Error(
                `pi-durable wrote its entries where the layer cannot join rows: ${sql}`,
            );
        }
    };

    const database: SqliteDatabase = {
        exec: (sql) => {
            refuseUnread(sql);
            return base.exec(sql);
        },
        run: (sql, ...params) => {
            refuseUnread(sql);
            return base.run(sql, ...params);
        },
        runCounted: (sql, ...params) => {
            refuseUnread(sql);
            return base.runCounted(sql, ...params);
        },
        get: (sql, ...params) => base.get(sql, ...params),
        all: (sql, ...params) => base.all(sql, ...params),
        transaction: async (callback) => {
            const settled: string[] = [];
            const result = await base.transaction((tx) =>
                callback(watched(tx, settled)),
            );
            for (const key of settled) {
                pending.delete(key);
            }
            return result;
        },
        close: () => base.close(),
    };

    return {
        database,
        buffer: (call, rows) => {
            pending.set(callKey(call.conversationId, call.callId), rows);
        },
        get buffered() {
            let count = 0;
            for (const rows of pending.values()) {
                count += rows.length;
            }
            return count;
        },
        follow: (session) => {
            void session.closed.then(() => pending.clear());
        },
    };
}
