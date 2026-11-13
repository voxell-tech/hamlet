import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync, type SQLInputValue } from "node:sqlite";
import type { TestContext } from "node:test";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { createModels, fauxProvider } from "@earendil-works/pi-ai";
import type { StorageConformanceAssertions } from "@earendil-works/pi-durable/testing";
import type { CoreRegistry } from "@hamlet/actor-node";
import { decode, encode } from "cbor-x";
import type { AnyActorDefinition } from "../src/actor/actor/definition.ts";
import type { RawAccess } from "../src/actor/common/database/mod.ts";

export const context = BACKGROUND_CONTEXT;

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
 * Starts `kinds` on the native module's host in this process, each actor in
 * its own file at `actors/<kind>/<key>.sqlite` under `dir`. Needs the native
 * module built and copied beside its loader in `crates/hamlet_actor_node`.
 */
export async function startHost(
    dir: string,
    kinds: Record<string, AnyActorDefinition>,
) {
    // Loaded here, not at the top: the layer's registry is large, and every
    // test that starts no host, kill children included, would load it.
    const [
        { RegistryConfigSchema },
        { buildNativeFactory },
        { loadNapiRuntime },
    ] = await Promise.all([
        import("../src/actor/registry/config/index.ts"),
        import("../src/actor/registry/native.ts"),
        import("../src/actor/registry/napi-runtime.ts"),
    ]);
    const { runtime } = await loadNapiRuntime();
    const config = RegistryConfigSchema.parse({ use: kinds });
    const handle = runtime.createRegistry();
    for (const [kind, definition] of Object.entries(kinds)) {
        runtime.registerActor(
            handle,
            kind,
            buildNativeFactory(runtime, config, definition),
        );
    }
    const registry = handle as unknown as CoreRegistry;
    const due = await registry.start(dir);
    return {
        /** The actors the host started at once: an alarm was due, or a kill cut off their run. */
        due,
        /** Calls `action` on the actor of `kind` and `key`, starting it if it is not running. */
        call: async (
            kind: string,
            key: string,
            action: string,
            ...args: unknown[]
        ): Promise<unknown> =>
            decode(await registry.action(kind, key, action, encode(args))),
        shutdown: () => registry.shutdown(),
    };
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
