import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import type { CoreRegistry } from "@hamlet/actor-node";
import { decode, encode } from "cbor-x";
import { actor } from "../../src/actor/actor/mod.ts";
import { db } from "../../src/actor/db/mod.ts";
import { RegistryConfigSchema } from "../../src/actor/registry/config/index.ts";
import { loadNapiRuntime } from "../../src/actor/registry/napi-runtime.ts";
import { buildNativeFactory } from "../../src/actor/registry/native.ts";

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

/** Starts the module's local registry in `dir` with the one kind `counter`. */
async function start(dir: string): Promise<CoreRegistry> {
    const { runtime } = await loadNapiRuntime();
    const config = RegistryConfigSchema.parse({ use: { counter } });
    const handle = runtime.createRegistry();
    runtime.registerActor(
        handle,
        "counter",
        buildNativeFactory(runtime, config, counter),
    );
    const registry = handle as unknown as CoreRegistry;
    await registry.start(dir);
    return registry;
}

const DEADLINE_MS = 30_000;

async function hit(registry: CoreRegistry, key = "a1"): Promise<unknown> {
    return decode(
        await registry.call("counter", key, "hit", encode([]), DEADLINE_MS),
    );
}

/** Asserts that `promise` fails with the actor error `code`. */
async function failsWith(promise: Promise<unknown>, code: string) {
    await assert.rejects(promise, (error: Error) =>
        error.message.includes(`"code":"${code}"`),
    );
}

async function inFolder(body: (dir: string) => Promise<void>) {
    const dir = mkdtempSync(join(tmpdir(), "hamlet-node-host-"));
    try {
        await body(dir);
    } finally {
        rmSync(dir, { recursive: true, force: true });
    }
}

test("an actor made by create answers calls with its input, and its rows outlive the registry", async () => {
    await inFolder(async (dir) => {
        const first = await start(dir);
        await first.create("counter", "a1", encode({ start: 10 }));
        assert.equal(await hit(first), 11);
        assert.equal(await hit(first), 12);
        await first.shutdown();

        const second = await start(dir);
        assert.equal(await hit(second), 13);
        await second.shutdown();
    });
});

test("a call never makes its target, and a create of a key that exists fails", async () => {
    await inFolder(async (dir) => {
        const registry = await start(dir);
        await failsWith(hit(registry, "a2"), "not_found");
        assert.equal(
            existsSync(join(dir, "actors", "counter", "a2.sqlite")),
            false,
        );
        await registry.create("counter", "a2", encode({ start: 0 }));
        await failsWith(
            registry.create("counter", "a2", encode({ start: 5 })),
            "duplicate_key",
        );
        assert.equal(await hit(registry, "a2"), 1);
        await registry.shutdown();
    });
});

test("getOrStart makes the actor a test calls", async () => {
    await inFolder(async (dir) => {
        const registry = await start(dir);
        await registry.getOrStart("counter", "a3");
        assert.equal(await hit(registry, "a3"), 1);
        await registry.shutdown();
    });
});
