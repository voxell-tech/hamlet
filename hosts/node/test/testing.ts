import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync, type SQLInputValue } from "node:sqlite";
import type { TestContext } from "node:test";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { createModels, fauxProvider } from "@earendil-works/pi-ai";
import type { StorageConformanceAssertions } from "@earendil-works/pi-durable/testing";
import type { RawAccess } from "../src/actor/common/database/mod.ts";
import { decodeCborCompat, decodeCborJsonCompat } from "../src/actor/serde.ts";

export const context = BACKGROUND_CONTEXT;

/** The folder of `hamlet_messages`' fixtures. */
export const FIXTURES = new URL(
    "../../../crates/hamlet_messages/fixtures/",
    import.meta.url,
);

/**
 * A fixture as the host decodes it: an answer as `Calls.call` does, and a
 * call's arguments or a create input as the host does for an action.
 */
export function fixture(name: string): unknown {
    const bytes = readFileSync(new URL(`${name}.cbor`, FIXTURES));
    return name.endsWith(".answer")
        ? decodeCborJsonCompat(bytes)
        : decodeCborCompat(bytes);
}

/** A folder removed after the test or suite whose `after` this is given. */
export function scratchFolder(t: { after(fn: () => void): void }): string {
    const folder = mkdtempSync(join(tmpdir(), "hamlet-agent-"));
    t.after(() => rmSync(folder, { recursive: true, force: true }));
    return folder;
}

/** A path for an agent's file in a folder removed after the test. */
export function scratchFile(t: TestContext): string {
    return join(scratchFolder(t), "agent.sqlite");
}

/** A faux model provider, with its model by reference and by the name `pi()` takes. */
export function fauxModels() {
    const faux = fauxProvider();
    const models = createModels();
    models.setProvider(faux.provider);
    const { provider, id } = faux.getModel();
    return {
        faux,
        models,
        model: `${provider}/${id}`,
        ref: { provider, modelId: id },
    };
}

/** The tables a SQLite file holds, by name, read while nothing else has it open. */
export function tablesIn(file: string): string[] {
    const database = new DatabaseSync(file, { readOnly: true });
    try {
        return database
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
            )
            .all()
            .map((row) => String(row.name));
    } finally {
        database.close();
    }
}

/** pi-durable 1.5.2's storage conformance suite has 32 cases. */
export const CONFORMANCE_CASES = 32;

export const conformanceAssertions: StorageConformanceAssertions = {
    ok: (value, message) => assert.ok(value, message),
    strictEqual: (actual, expected) => assert.strictEqual(actual, expected),
    deepEqual: (actual, expected) => assert.deepStrictEqual(actual, expected),
    partialDeepEqual: (actual, expected) =>
        assert.partialDeepStrictEqual(actual, expected),
    greaterThan: (actual, expected) =>
        assert.ok(actual > expected, `${actual} is not above ${expected}`),
    rejects: (operation, messageIncludes) =>
        assert.rejects(operation, (error: unknown) => {
            const message = String((error as Error)?.message ?? error);
            assert.ok(
                message.includes(messageIncludes),
                `${message} does not include ${messageIncludes}`,
            );
            return true;
        }),
};

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
    const access = (turn: <T>(work: () => T) => Promise<T>) => {
        const execute = <TRow extends Record<string, unknown>>(
            sql: string,
            ...args: unknown[]
        ) => turn(() => run<TRow>(sql, args));
        return {
            execute,
            all: execute,
            get: <TRow extends Record<string, unknown>>(
                sql: string,
                ...args: unknown[]
            ) => turn(() => run<TRow>(sql, args).at(0)),
            run: (sql: string, ...args: unknown[]) =>
                turn(() => {
                    const { changes, lastInsertRowid } = db
                        .prepare(sql)
                        .run(...(args as SQLInputValue[]));
                    return {
                        changes: Number(changes),
                        lastInsertRowId: Number(lastInsertRowid),
                    };
                }),
        };
    };
    const inTransaction: RawAccess = {
        ...access(async (work) => work()),
        transaction: () => {
            throw new Error("nested transaction");
        },
        close: async () => {},
    };
    return {
        ...access(inTurn),
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
