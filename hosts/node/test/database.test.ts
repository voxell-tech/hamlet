import assert from "node:assert/strict";
import { test } from "node:test";
import type {
    SqliteBindings,
    SqliteDatabase,
} from "../src/actor/common/database/config.ts";
import { db } from "../src/actor/db/mod.ts";

type Result = Awaited<ReturnType<SqliteDatabase["execute"]>>;

/** A database whose every statement answers `answers[sql]`, and logs what it ran. */
function fake(answers: Record<string, Result>) {
    const ran: [string, string, SqliteBindings | undefined][] = [];
    const on =
        (where: string) => async (sql: string, params?: SqliteBindings) => {
            ran.push([where, sql, params]);
            const answer = answers[sql];
            assert.ok(answer, sql);
            return answer;
        };
    const tx = {
        execute: on("tx"),
        commit: async () => {},
        rollback: async () => {},
    };
    const database = {
        execute: on("db"),
        beginTransaction: async () => tx,
        close: async () => {},
    } as unknown as SqliteDatabase;
    return { database, ran };
}

const NOTES = {
    "INSERT INTO notes (text) VALUES (?)": {
        columns: [],
        rows: [],
        changes: 1,
        lastInsertRowId: 7,
    },
    "UPDATE notes SET done = 1 WHERE id = :id": {
        columns: [],
        rows: [],
        changes: 2,
        lastInsertRowId: 7,
    },
    "SELECT id, text FROM notes": {
        columns: ["id", "text"],
        rows: [
            [1, "milk"],
            [2, "eggs"],
        ],
        changes: 0,
        lastInsertRowId: null,
    },
    "SELECT id FROM notes WHERE id = ?": {
        columns: ["id"],
        rows: [],
        changes: 0,
    },
} satisfies Record<string, Result>;

const unused = async (): Promise<never> => {
    throw new Error("the native database uses no KV");
};
const NO_KV = {
    batchPut: unused,
    batchGet: unused,
    batchDelete: unused,
    deleteRange: unused,
};

test("get, all and run read one statement on the client and on a transaction", async () => {
    const { database, ran } = fake(NOTES);
    const client = await db().createClient({
        actorId: "notes/n1",
        kv: NO_KV,
        nativeDatabaseProvider: { open: async () => database },
    });

    assert.deepEqual(
        await client.run("INSERT INTO notes (text) VALUES (?)", "milk"),
        { changes: 1, lastInsertRowId: 7 },
    );
    assert.deepEqual(await client.run("SELECT id, text FROM notes"), {
        changes: 0,
        lastInsertRowId: null,
    });
    assert.deepEqual(await client.run("SELECT id FROM notes WHERE id = ?", 9), {
        changes: 0,
        lastInsertRowId: null,
    });
    assert.deepEqual(await client.get("SELECT id, text FROM notes"), {
        id: 1,
        text: "milk",
    });
    assert.equal(
        await client.get("SELECT id FROM notes WHERE id = ?", 9),
        undefined,
    );
    assert.deepEqual(await client.all("SELECT id, text FROM notes"), [
        { id: 1, text: "milk" },
        { id: 2, text: "eggs" },
    ]);

    const inTx = await client.transaction(async (tx) => ({
        run: await tx.run("UPDATE notes SET done = 1 WHERE id = :id", {
            id: 2,
        }),
        get: await tx.get("SELECT id, text FROM notes"),
        all: await tx.all("SELECT id FROM notes WHERE id = ?", 9),
    }));
    assert.deepEqual(inTx, {
        run: { changes: 2, lastInsertRowId: 7 },
        get: { id: 1, text: "milk" },
        all: [],
    });

    assert.deepEqual(ran, [
        ["db", "INSERT INTO notes (text) VALUES (?)", ["milk"]],
        ["db", "SELECT id, text FROM notes", undefined],
        ["db", "SELECT id FROM notes WHERE id = ?", [9]],
        ["db", "SELECT id, text FROM notes", undefined],
        ["db", "SELECT id FROM notes WHERE id = ?", [9]],
        ["db", "SELECT id, text FROM notes", undefined],
        ["tx", "UPDATE notes SET done = 1 WHERE id = :id", { id: 2 }],
        ["tx", "SELECT id, text FROM notes", undefined],
        ["tx", "SELECT id FROM notes WHERE id = ?", [9]],
    ]);
});
