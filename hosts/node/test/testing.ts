import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync, type SQLInputValue } from "node:sqlite";
import type { TestContext } from "node:test";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { createModels, fauxProvider } from "@earendil-works/pi-ai";
import {
    createRegistry,
    defineExtension,
    defineTool,
    Harness,
    type Storage,
} from "@earendil-works/pi-durable";
import {
    type SqliteDatabase,
    SqliteStorage,
} from "@earendil-works/pi-durable/storage/sqlite";
import type { RawAccess } from "../src/actor/common/database/mod.ts";
import {
    type AgentDatabase,
    agentDatabase,
    type WriteRow,
} from "../src/agent/database.ts";
import { actorSqlite } from "../src/pi/actor-sqlite.ts";

export const context = BACKGROUND_CONTEXT;

/** A path for an agent's file in a folder removed after the test. */
export function scratchFile(t: TestContext): string {
    const folder = mkdtempSync(join(tmpdir(), "hamlet-agent-"));
    t.after(() => rmSync(folder, { recursive: true, force: true }));
    return join(folder, "agent.sqlite");
}

/** A letter the `tell` tool sends, named by the call that sends it. */
export type Letter = {
    readonly call: string;
    readonly to: string;
    readonly body: string;
};

/**
 * A stand-in for the actor's `send`, which TypeScript does not have yet: a
 * plain table, written with the transaction handle the layer passes.
 */
export const writeLetter: WriteRow<Letter> = (tx, letter) =>
    tx.run(
        "INSERT INTO stand_in_sends (call, recipient, body) VALUES (?, ?, ?)",
        letter.call,
        letter.to,
        letter.body,
    );

/** What `tell` is asked to do, as the faux model's tool call gives it. */
export interface TellArguments {
    readonly to: string;
    readonly body: string;
    /** Throw after the letter is buffered. */
    readonly fail?: boolean;
    /** Pad the tool's output to this many characters. */
    readonly size?: number;
}

function tellArguments(args: unknown): Required<TellArguments> {
    const { to, body, fail, size } = args as Record<string, unknown>;
    if (typeof to !== "string" || typeof body !== "string") {
        throw new Error("tell needs `to` and `body`");
    }
    return {
        to,
        body,
        fail: fail === true,
        size: typeof size === "number" ? size : 0,
    };
}

export interface AgentOptions {
    /** Wraps the database the layer writes through, below the layer. */
    readonly base?: (database: SqliteDatabase) => SqliteDatabase;
    /** Wraps pi-durable's storage, above the layer. */
    readonly storage?: (
        storage: Storage,
        layer: AgentDatabase<Letter>,
    ) => Storage;
    /** Runs in `tell`, right after it buffered its letter. */
    readonly afterBuffer?: () => void;
}

/**
 * A pi-durable Harness on Hamlet's database layer, over the wrapper's facade
 * on a plain SQLite file, with one faux model and a `tell` tool that buffers
 * a letter for the commit of its result.
 */
export async function openAgent(file: string, options: AgentOptions = {}) {
    const database = plainDatabase(file);
    await database.execute(
        "CREATE TABLE IF NOT EXISTS stand_in_sends (call TEXT PRIMARY KEY, recipient TEXT NOT NULL, body TEXT NOT NULL)",
    );
    const facade = actorSqlite(database);
    const layer = agentDatabase(options.base?.(facade) ?? facade, writeLetter);
    const tell = defineTool({
        name: "tell",
        description: "Send a letter.",
        parameters: {
            type: "object",
            required: ["to", "body"],
            properties: {
                to: { type: "string" },
                body: { type: "string" },
                fail: { type: "boolean" },
                size: { type: "number" },
            },
        },
        replay: "safe",
        outputLimits: { maxBytes: 1024 * 1024 },
        execute: async (args, api) => {
            const { to, body, fail, size } = tellArguments(args);
            layer.buffer(api, [{ call: api.callId, to, body }]);
            options.afterBuffer?.();
            if (fail) {
                throw new Error("tell failed after it buffered its letter");
            }
            return {
                output: [{ type: "text", text: "sent".padEnd(size, ".") }],
            };
        },
    });
    const registry = createRegistry();
    registry.install(defineExtension({ name: "letters", tools: [tell] }));
    const faux = fauxProvider();
    const models = createModels();
    models.setProvider(faux.provider);
    const sqlite = await SqliteStorage.open(layer.database);
    const storage = options.storage?.(sqlite, layer) ?? sqlite;
    const harness = await Harness.open(storage, { models, registry }, context);
    layer.follow(harness);
    const model = faux.getModel();
    const root = await harness.root(context, {
        agent: { model: { provider: model.provider, modelId: model.id } },
    });
    const close = async () => {
        await harness.close(context);
        await database.close();
    };
    return { database, layer, faux, harness, root, close };
}

/** One `tell` result in the file. */
export interface StoredResult {
    readonly call: string;
    readonly isError: boolean;
}

/** `tell`'s results and the stand-in's letters, read with plain SQL. */
export async function stored(database: Pick<RawAccess, "execute">) {
    const entries = await database.execute<{ record: string }>(
        "SELECT record FROM pi_entries ORDER BY id",
    );
    const results: StoredResult[] = entries.flatMap((row) => {
        const record = JSON.parse(row.record) as {
            kind: string;
            model?: {
                toolCallId: string;
                toolName: string;
                isError: boolean;
            }[];
        };
        if (record.kind !== "pi.tool-result") {
            return [];
        }
        return (record.model ?? [])
            .filter((message) => message.toolName === "tell")
            .map((message) => ({
                call: message.toolCallId,
                isError: message.isError,
            }));
    });
    const letters = await database.execute<Letter>(
        'SELECT call, recipient AS "to", body FROM stand_in_sends ORDER BY call',
    );
    return { results, letters: letters.map((letter) => ({ ...letter })) };
}

/**
 * The actor layer's database client over a plain `node:sqlite` file, with no
 * actor under it. Calls run one at a time, and while a transaction is open
 * every other call waits for it, as the actor's own client does.
 */
export function plainDatabase(path: string): RawAccess {
    const db = new DatabaseSync(path);
    let last: Promise<unknown> = Promise.resolve();
    const inTurn = <T>(work: () => Promise<T> | T): Promise<T> => {
        const next = last.then(work, work);
        last = next.catch(() => undefined);
        return next;
    };
    const run = <TRow>(sql: string, args: unknown[]): TRow[] => {
        const statement = db.prepare(sql);
        if (statement.columns().length > 0) {
            return statement.all(...(args as SQLInputValue[])) as TRow[];
        }
        if (args.length === 0) {
            // `prepare` compiles only the first statement, so a script with
            // no rows to return runs whole.
            db.exec(sql);
        } else {
            statement.run(...(args as SQLInputValue[]));
        }
        return [];
    };
    const inTransaction: RawAccess = {
        execute: async (sql, ...args) => run(sql, args),
        transaction: () => {
            throw new Error("nested transaction");
        },
        close: async () => {},
    };
    return {
        execute: (sql, ...args) => inTurn(() => run(sql, args)),
        transaction: (callback) =>
            inTurn(async () => {
                db.exec("BEGIN IMMEDIATE");
                try {
                    const result = await callback(inTransaction);
                    db.exec("COMMIT");
                    return result;
                } catch (error) {
                    db.exec("ROLLBACK");
                    throw error;
                }
            }),
        close: async () => db.close(),
    };
}
