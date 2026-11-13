import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { describe, test } from "node:test";
import { openNodeSqliteDatabase } from "@earendil-works/chord/sqlite/node";
import {
    type SqliteDatabase,
    SqliteStorage,
} from "@earendil-works/pi-durable/storage/sqlite";
import {
    createStorageConformance,
    type StorageConformanceAssertions,
} from "@earendil-works/pi-durable/testing";
import { agentDatabase } from "../src/agent/database.ts";
import { actorSqlite } from "../src/pi/actor-sqlite.ts";
import { context, plainDatabase, writeLetter } from "./testing.ts";

/** pi-durable 1.5.2's storage conformance suite has 32 cases. */
const CASES = 32;

const assertions: StorageConformanceAssertions = {
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

/** The tables a SQLite file holds. */
function tables(file: string): string[] {
    const database = new DatabaseSync(file, { readOnly: true });
    try {
        return database
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .all()
            .map((row) => String(row.name));
    } finally {
        database.close();
    }
}

/**
 * pi-durable's storage on Hamlet's database layer over `open`'s database,
 * once per case. `tablesOf` gets the tables each case left.
 */
function cases(
    open: (file: string) => Promise<SqliteDatabase>,
    tablesOf: (names: string[]) => void,
) {
    return createStorageConformance({
        assertions,
        withStorage: async (use) => {
            const folder = mkdtempSync(join(tmpdir(), "hamlet-conformance-"));
            const file = join(folder, "agent.sqlite");
            try {
                const layer = agentDatabase(await open(file), writeLetter);
                // Closing the storage closes the database under it.
                const storage = await SqliteStorage.open(layer.database);
                try {
                    await use(storage);
                } finally {
                    await storage.close(context);
                }
                tablesOf(tables(file));
            } finally {
                rmSync(folder, { recursive: true, force: true });
            }
        },
    });
}

describe("pi-durable's storage conformance on the layer over the wrapper's facade, tables prefixed pi_", () => {
    const seen = new Set<string>();
    const list = cases(
        async (file) => {
            const database = plainDatabase(file);
            const facade = actorSqlite(database);
            return { ...facade, close: () => database.close() };
        },
        (names) => {
            for (const name of names) {
                seen.add(name);
            }
        },
    );
    test("has every case", () => assert.equal(list.length, CASES));
    for (const each of list) {
        test(each.name, () => each.run());
    }
    test("kept pi-durable's tables under pi_", () => {
        assert.ok(seen.has("pi_entries"), [...seen].join(", "));
        assert.deepEqual(
            [...seen].filter((name) => !name.startsWith("pi_")),
            [],
        );
    });
});

describe("pi-durable's storage conformance on the layer over node:sqlite, tables unprefixed", () => {
    const seen = new Set<string>();
    const list = cases(
        (file) => openNodeSqliteDatabase(file),
        (names) => {
            for (const name of names) {
                seen.add(name);
            }
        },
    );
    test("has every case", () => assert.equal(list.length, CASES));
    for (const each of list) {
        test(each.name, () => each.run());
    }
    test("kept pi-durable's tables unprefixed", () => {
        assert.ok(seen.has("entries"), [...seen].join(", "));
        assert.deepEqual(
            [...seen].filter((name) => name.startsWith("pi_")),
            [],
        );
    });
});
