// This file was changed for Hamlet. See NOTICE in this package.
import type { ConversationId } from "@earendil-works/pi-durable";
import type { RawAccess } from "../actor/db/mod.ts";
import { migrations } from "../actor/unstable/migrations.ts";
import { PI_DURABLE_TABLES } from "./actor-sqlite.ts";

export type PiDatabase = Pick<RawAccess, "execute">;

/**
 * `pi()`'s own tables. Pi Durable creates its tables in the same database
 * itself, with the `pi_` prefix `actorSqlite` adds. The `pi_durable` names are
 * stored in actors' databases, so they stay.
 */
export const migratePiTables = migrations({
    tableName: "pi_durable_schema_version",
    migrations: [
        {
            version: 1,
            sql: `
				CREATE TABLE pi_durable_watch (
					conn_id TEXT NOT NULL,
					watch_key TEXT NOT NULL,
					spec_json TEXT NOT NULL CHECK (json_valid(spec_json)),
					PRIMARY KEY (conn_id, watch_key)
				) STRICT, WITHOUT ROWID;
			`,
        },
        {
            version: 2,
            // Files of the database env. Agents from 0.5.1 created this table on first use.
            sql: `
				CREATE TABLE IF NOT EXISTS pi_file (
					path TEXT PRIMARY KEY,
					content BLOB NOT NULL,
					mtime_ms INTEGER NOT NULL
				) STRICT;
			`,
        },
        {
            version: 3,
            // Pi Durable's tables of agents from 0.5.1 and earlier have no prefix. Renaming them keeps their data.
            up: async (db) => {
                for (const table of PI_DURABLE_TABLES) {
                    const [found] = await db.execute(
                        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?",
                        table,
                    );
                    if (found)
                        await db.execute(
                            `ALTER TABLE ${table} RENAME TO pi_${table}`,
                        );
                }
            },
        },
    ],
});

/** A watch a connection asked for. Stored so a connection that survives sleep gets its watches back on wake. */
export type StoredWatch = {
    connId: string;
    key: string;
    /** `events` watches were saved before 0.5.6. Reattaching deletes them. */
    spec: WatchSpec | { kind: "events" };
};

export type WatchSpec =
    | { kind: "view"; conversationId: ConversationId }
    | { kind: "taskGraph" }
    | { kind: "doc"; docKind: string; conversationId: ConversationId };

export async function saveWatch(
    db: PiDatabase,
    watch: StoredWatch,
): Promise<void> {
    await db.execute(
        `INSERT INTO pi_durable_watch (conn_id, watch_key, spec_json) VALUES (?, ?, ?)
		 ON CONFLICT (conn_id, watch_key) DO NOTHING`,
        watch.connId,
        watch.key,
        JSON.stringify(watch.spec),
    );
}

export async function deleteWatch(
    db: PiDatabase,
    connId: string,
    key: string,
): Promise<void> {
    await db.execute(
        `DELETE FROM pi_durable_watch WHERE conn_id = ? AND watch_key = ?`,
        connId,
        key,
    );
}

export async function deleteConnectionWatches(
    db: PiDatabase,
    connId: string,
): Promise<void> {
    await db.execute(`DELETE FROM pi_durable_watch WHERE conn_id = ?`, connId);
}

export async function loadWatches(db: PiDatabase): Promise<StoredWatch[]> {
    const rows = await db.execute<{
        conn_id: string;
        watch_key: string;
        spec_json: string;
    }>(`SELECT conn_id, watch_key, spec_json FROM pi_durable_watch`);
    return rows.map((row) => ({
        connId: row.conn_id,
        key: row.watch_key,
        // Only `saveWatch` writes this column, from a `StoredWatch`.
        spec: JSON.parse(row.spec_json) as StoredWatch["spec"],
    }));
}
