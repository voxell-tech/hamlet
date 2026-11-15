import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import {
    actor,
    type CallOptions,
    isActorErrorCode,
} from "../../src/actor/actor/mod.ts";
import { db } from "../../src/actor/db/mod.ts";
import { inFolder, type LocalHost, startLocalHost } from "./support.ts";

const counter = actor({
    db: db({
        onMigrate: async (raw) => {
            await raw.execute(
                "CREATE TABLE IF NOT EXISTS base (start INTEGER NOT NULL)",
            );
            await raw.execute(
                "CREATE TABLE IF NOT EXISTS hits (n INTEGER PRIMARY KEY)",
            );
        },
    }),
    onCreate: async (c, input?: { start: number }) => {
        await c.db.execute(
            "INSERT INTO base (start) VALUES (?)",
            input?.start ?? 0,
        );
    },
    actions: {
        hit: async (c) => {
            await c.db.execute("INSERT INTO hits DEFAULT VALUES");
            const [row] = await c.db.execute<{ total: number }>(
                "SELECT (SELECT start FROM base) + count(*) AS total FROM hits",
            );
            return row?.total;
        },
    },
});

const CALL: CallOptions = { deadlineMs: 30_000 };

const KINDS = { counter };

function hit(host: LocalHost<typeof KINDS>, key = "a1") {
    return host.call(host.at("counter", key), "hit", [], CALL);
}

async function failsWith(promise: Promise<unknown>, code: string) {
    await assert.rejects(promise, (error) =>
        isActorErrorCode(error, "actor", code),
    );
}

test("an actor made by create answers calls with its input, and its rows outlive the registry", async () => {
    await inFolder(async (dir) => {
        const first = await startLocalHost(dir, KINDS);
        await first.create(first.at("counter", "a1"), { start: 10 });
        assert.equal(await hit(first), 11);
        assert.equal(await hit(first), 12);
        await first.shutdown();

        const second = await startLocalHost(dir, KINDS);
        assert.equal(await hit(second), 13);
        await second.shutdown();
    });
});

test("a call never makes its target, and a create of a key that exists fails", async () => {
    await inFolder(async (dir) => {
        const host = await startLocalHost(dir, KINDS);
        const a2 = host.at("counter", "a2");
        await failsWith(hit(host, "a2"), "not_found");
        assert.equal(
            existsSync(join(dir, "actors", "counter", "a2.sqlite")),
            false,
        );
        await host.create(a2, { start: 0 });
        assert.equal(
            existsSync(join(dir, "actors", "counter", "a2.sqlite")),
            true,
        );
        await failsWith(host.create(a2, { start: 5 }), "duplicate_key");
        assert.equal(await hit(host, "a2"), 1);
        await host.shutdown();
    });
});

test("getOrStart makes the actor a test calls", async () => {
    await inFolder(async (dir) => {
        const host = await startLocalHost(dir, KINDS);
        await host.getOrStart(host.at("counter", "a3"));
        assert.equal(await hit(host, "a3"), 1);
        await host.shutdown();
    });
});
