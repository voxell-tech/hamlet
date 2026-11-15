import assert from "node:assert/strict";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import {
    actor,
    type CallOptions,
    isActorErrorCode,
    UserError,
} from "../../src/actor/actor/mod.ts";
import { db } from "../../src/actor/db/mod.ts";
import { eventually, inFolder, startLocalHost } from "./support.ts";

const CALL: CallOptions = { deadlineMs: 10_000 };
const DOWN_PAST_THE_FIRE_MS = 800;
const ROOM_FOR_A_SECOND_FIRE_MS = 300;

type Note = { id: number; text: string; done: number };

const notes = actor({
    db: db({
        onMigrate: async (raw) => {
            await raw.execute(
                "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, text TEXT NOT NULL, done INTEGER NOT NULL DEFAULT 0)",
            );
        },
    }),
    actions: {
        add: async (c, text: string) => {
            const { lastInsertRowId } = await c.db.run(
                "INSERT INTO notes (text) VALUES (?)",
                text,
            );
            return { id: lastInsertRowId };
        },
        finish: async (c, ids: readonly number[]) =>
            await c.db.transaction(async (tx) => {
                let changed = 0;
                for (const id of ids) {
                    const { changes } = await tx.run(
                        "UPDATE notes SET done = 1 WHERE id = ? AND done = 0",
                        id,
                    );
                    changed += changes;
                }
                const open = await tx.get<{ n: number }>(
                    "SELECT count(*) AS n FROM notes WHERE done = 0",
                );
                return { changed, open: open?.n ?? 0 };
            }),
        finishThenFail: async (c) =>
            await c.db.transaction(async (tx) => {
                await tx.run("UPDATE notes SET done = 1");
                const open = await tx.get<{ n: number }>(
                    "SELECT count(*) AS n FROM notes WHERE done = 0",
                );
                throw new UserError("changed my mind", {
                    code: "changed_mind",
                    metadata: { open: open?.n },
                });
            }),
        list: async (c) =>
            await c.db.all<Note>(
                "SELECT id, text, done FROM notes ORDER BY id",
            ),
        note: async (c, id: number) =>
            (await c.db.get<Note>(
                "SELECT id, text, done FROM notes WHERE id = ?",
                id,
            )) ?? null,
        runRead: async (c) => await c.db.run("SELECT 1"),
        runTwo: async (c) =>
            await c.db.run(
                "INSERT INTO notes (text) VALUES ('one'); INSERT INTO notes (text) VALUES ('two')",
            ),
    },
});

test("an actor's rows, their count and an inserted id outlive the action and the host", async () => {
    await inFolder(async (dir) => {
        const first = await startLocalHost(dir, { notes });
        const n1 = first.at("notes", "n1");
        await first.create(n1);
        assert.deepEqual(await first.call(n1, "add", ["milk"], CALL), {
            id: 1,
        });
        assert.deepEqual(await first.call(n1, "add", ["eggs"], CALL), {
            id: 2,
        });
        assert.deepEqual(await first.call(n1, "finish", [[1, 2, 2]], CALL), {
            changed: 2,
            open: 0,
        });
        assert.deepEqual(await first.call(n1, "finish", [[1]], CALL), {
            changed: 0,
            open: 0,
        });
        assert.equal(await first.call(n1, "note", [9], CALL), null);
        assert.deepEqual(await first.call(n1, "add", ["tea"], CALL), {
            id: 3,
        });
        await assert.rejects(
            first.call(n1, "finishThenFail", [], CALL),
            (error) =>
                isActorErrorCode(error, "user", "changed_mind") &&
                (error.metadata as { open?: unknown }).open === 0,
        );
        assert.deepEqual(await first.call(n1, "note", [3], CALL), {
            id: 3,
            text: "tea",
            done: 0,
        });
        await first.shutdown();

        const second = await startLocalHost(dir, { notes });
        assert.deepEqual(
            await second.call(second.at("notes", "n1"), "list", [], CALL),
            [
                { id: 1, text: "milk", done: 1 },
                { id: 2, text: "eggs", done: 1 },
                { id: 3, text: "tea", done: 0 },
            ],
        );
        await second.shutdown();
    });
});

test("run answers no id after a read, and refuses a text of two statements", async () => {
    await inFolder(async (dir) => {
        const host = await startLocalHost(dir, { notes });
        const n1 = host.at("notes", "n1");
        await host.create(n1);
        assert.deepEqual(await host.call(n1, "runRead", [], CALL), {
            changes: 0,
            lastInsertRowId: null,
        });
        await assert.rejects(host.call(n1, "runTwo", [], CALL), (error) =>
            isActorErrorCode(error, "core", "internal_error"),
        );
        assert.deepEqual(await host.call(n1, "list", [], CALL), []);
        await host.shutdown();
    });
});

const ticker = actor({
    db: db({
        onMigrate: async (raw) => {
            await raw.execute(
                "CREATE TABLE IF NOT EXISTS fired (n INTEGER NOT NULL, scheduled INTEGER NOT NULL, caller INTEGER NOT NULL)",
            );
        },
    }),
    actions: {
        arm: async (c, ms: number, n: number) =>
            await c.schedule.at(Date.now() + ms, "fire", n),
        fire: async (c, n: number) => {
            await c.db.run(
                "INSERT INTO fired (n, scheduled, caller) VALUES (?, ?, ?)",
                n,
                c.scheduledFire === undefined ? 0 : 1,
                c.caller === null ? 0 : 1,
            );
        },
        fired: async (c) =>
            await c.db.all<{ n: number; scheduled: number; caller: number }>(
                "SELECT n, scheduled, caller FROM fired ORDER BY rowid",
            ),
        pending: async (c) => (await c.schedule.list()).length,
    },
});

test("an actor's own schedule fires once, also when the host was down at the time", async () => {
    await inFolder(async (dir) => {
        const first = await startLocalHost(dir, { ticker });
        const t1 = first.at("ticker", "t1");
        await first.create(t1);
        await first.call(t1, "arm", [300, 7], CALL);
        assert.equal(await first.call(t1, "pending", [], CALL), 1);
        assert.deepEqual(
            await eventually(
                () => first.call(t1, "fired", [], CALL),
                (rows) => rows.length > 0,
            ),
            [{ n: 7, scheduled: 1, caller: 0 }],
        );
        assert.equal(await first.call(t1, "pending", [], CALL), 0);

        await first.call(t1, "arm", [400, 8], CALL);
        await first.shutdown();
        await sleep(DOWN_PAST_THE_FIRE_MS);
        const second = await startLocalHost(dir, { ticker });
        const rows = await eventually(
            () => second.call(second.at("ticker", "t1"), "fired", [], CALL),
            (rows) => rows.length > 1,
        );
        await sleep(ROOM_FOR_A_SECOND_FIRE_MS);
        assert.deepEqual(
            await second.call(second.at("ticker", "t1"), "fired", [], CALL),
            rows,
        );
        assert.deepEqual(
            rows.filter((row) => row.n === 8),
            [{ n: 8, scheduled: 1, caller: 0 }],
        );
        assert.equal(
            await second.call(second.at("ticker", "t1"), "pending", [], CALL),
            0,
        );
        await second.shutdown();
    });
});
