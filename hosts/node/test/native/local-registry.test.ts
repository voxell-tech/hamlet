import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
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
                "CREATE TABLE IF NOT EXISTS hits (n INTEGER PRIMARY KEY)",
            );
        },
    }),
    actions: {
        hit: async (c) => {
            await c.db.execute("INSERT INTO hits DEFAULT VALUES");
            const [row] = await c.db.execute<{ hits: number }>(
                "SELECT count(*) AS hits FROM hits",
            );
            return row?.hits;
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

async function hit(registry: CoreRegistry): Promise<unknown> {
    return decode(await registry.action("counter", "a1", "hit", encode([])));
}

test("a TypeScript actor on the local registry answers calls, and its rows outlive the registry", async () => {
    const dir = mkdtempSync(join(tmpdir(), "hamlet-node-host-"));
    try {
        const first = await start(dir);
        assert.equal(await hit(first), 1);
        assert.equal(await hit(first), 2);
        await first.shutdown();

        const second = await start(dir);
        assert.equal(await hit(second), 3);
        await second.shutdown();
    } finally {
        rmSync(dir, { recursive: true, force: true });
    }
});
