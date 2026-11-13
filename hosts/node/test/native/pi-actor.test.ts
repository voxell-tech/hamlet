import assert from "node:assert/strict";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";
import {
    fauxAssistantMessage,
    fauxText,
    fauxToolCall,
} from "@earendil-works/pi-ai";
import {
    createRegistry,
    defineExtension,
    defineTool,
} from "@earendil-works/pi-durable";
import { SqliteStorage } from "@earendil-works/pi-durable/storage/sqlite";
import { createStorageConformance } from "@earendil-works/pi-durable/testing";
import { actor } from "../../src/actor/actor/mod.ts";
import { db } from "../../src/actor/db/mod.ts";
import { agentDatabase } from "../../src/agent/database.ts";
import { pi } from "../../src/pi/actor.ts";
import { actorSqlite } from "../../src/pi/actor-sqlite.ts";
import { piActor } from "../../src/pi/runtime.ts";
import {
    CONFORMANCE_CASES,
    conformanceAssertions,
    context,
    fauxModels,
    scratchFolder,
    startHost,
    tablesIn,
    writeLetter,
} from "../testing.ts";

/** The tables the pi wrapper keeps in an agent's file, beside the actor core's. */
const PI_TABLES = [
    "pi_conversations",
    "pi_document_revisions",
    "pi_documents",
    "pi_durable_metadata",
    "pi_durable_schema",
    "pi_durable_schema_version",
    "pi_durable_watch",
    "pi_entries",
    "pi_file",
    "pi_record_ids",
    "pi_submissions",
    "pi_tasks",
];

/** The actor core's own tables share the file. */
const CORE_TABLE = /^_(actor|hamlet)_/;

test("the pi wrapper as an actor answers one prompt once, and a second prompt with its request id changes nothing", async (t) => {
    const dir = scratchFolder(t);
    const { faux, models, model } = fauxModels();
    faux.setResponses([fauxAssistantMessage([fauxText("Paris.")])]);
    const agent = pi({
        registry: createRegistry(),
        models,
        model,
        actions: {
            transcript: async (c) => ({
                entries: await c.db.execute(
                    "SELECT *, record ->> '$.kind' AS kind FROM pi_entries ORDER BY id",
                ),
                submissions: await c.db.execute(
                    "SELECT * FROM pi_submissions ORDER BY id",
                ),
            }),
        },
    });
    const host = await startHost(dir, { agent });
    const prompt = [
        "What is the capital of France?",
        { requestId: "prompt-1" },
    ];

    const answered = await host.call("agent", "a1", "prompt", ...prompt);
    assert.partialDeepStrictEqual(answered, { status: "done", text: "Paris." });
    assert.equal(faux.state.callCount, 1);
    const transcript = (await host.call("agent", "a1", "transcript")) as {
        entries: { kind: string }[];
    };
    assert.deepEqual(
        transcript.entries
            .map((row) => row.kind)
            .filter((kind) => kind === "pi.user" || kind === "pi.assistant"),
        ["pi.user", "pi.assistant"],
    );

    const again = await host.call("agent", "a1", "prompt", ...prompt);
    assert.deepEqual(again, answered);
    assert.equal(faux.state.callCount, 1);
    assert.deepEqual(await host.call("agent", "a1", "transcript"), transcript);
    await host.shutdown();

    assert.deepEqual(
        tablesIn(join(dir, "actors", "agent", "a1.sqlite")).filter(
            (name) => !CORE_TABLE.test(name),
        ),
        PI_TABLES,
    );
});

test("each tool and environment of the actor gets the context its onWake got", async (t) => {
    const dir = scratchFolder(t);
    const { faux, models, model } = fauxModels();
    faux.setResponses([
        fauxAssistantMessage([fauxToolCall("whoami", {})], {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("done"),
    ]);
    const woken: unknown[] = [];
    const seen: { by: "env" | "tool"; actor: unknown }[] = [];
    const whoami = defineTool({
        name: "whoami",
        description: "Says which actor runs it.",
        parameters: { type: "object", properties: {} },
        execute: async (_args, _api, toolContext) => {
            seen.push({ by: "tool", actor: piActor(toolContext) });
            return { output: [{ type: "text", text: "me" }] };
        },
    });
    const registry = createRegistry();
    registry.install(defineExtension({ name: "who", tools: [whoami] }));
    const agent = pi({
        registry,
        models,
        model,
        env: (_target, envContext) => {
            seen.push({ by: "env", actor: piActor(envContext) });
            return undefined;
        },
        onWake: (c) => {
            woken.push(c);
        },
    });
    const host = await startHost(dir, { agent });

    const answered = await host.call("agent", "a1", "prompt", "Who are you?");
    await host.shutdown();

    assert.partialDeepStrictEqual(answered, { status: "done", text: "done" });
    assert.equal(woken.length, 1);
    assert.deepEqual(
        seen.map(({ by, actor }) => [by, actor === woken[0]]),
        [
            ["env", true],
            ["env", true],
            ["tool", true],
            ["env", true],
        ],
    );
});

/**
 * An actor that runs one case of pi-durable's storage conformance suite on its
 * own `c.db`, under Hamlet's database layer over the wrapper's facade, and
 * returns the tables it left or how the case failed.
 */
const conformance = actor({
    db: db(),
    actions: {
        run: async (c, name: string) => {
            const [each] = createStorageConformance({
                assertions: conformanceAssertions,
                withStorage: async (use) => {
                    const layer = agentDatabase(actorSqlite(c.db), writeLetter);
                    const storage = await SqliteStorage.open(layer.database);
                    try {
                        await use(storage);
                    } finally {
                        await storage.close(context);
                    }
                },
            }).filter((candidate) => candidate.name === name);
            if (each === undefined) return { failed: `no case ${name}` };
            try {
                await each.run();
            } catch (error) {
                return {
                    failed:
                        error instanceof Error
                            ? (error.stack ?? error.message)
                            : String(error),
                };
            }
            const tables = await c.db.execute<{ name: string }>(
                "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
            );
            return { tables: tables.map((row) => row.name) };
        },
    },
});

describe("pi-durable's storage conformance on c.db inside an actor, under the layer over the wrapper's facade, tables prefixed pi_", () => {
    const names = createStorageConformance({
        assertions: conformanceAssertions,
        withStorage: () => {
            throw new Error("only the names are read here");
        },
    }).map((each) => each.name);
    let host: Awaited<ReturnType<typeof startHost>>;
    after(() => host.shutdown());
    const dir = scratchFolder({ after });
    before(async () => {
        host = await startHost(dir, { conformance });
    });

    test("has every case", () => assert.equal(names.length, CONFORMANCE_CASES));
    for (const [index, name] of names.entries()) {
        test(name, async () => {
            // A fresh actor, so each case gets a file of its own.
            const outcome = (await host.call(
                "conformance",
                `case-${index}`,
                "run",
                name,
            )) as { failed?: string; tables?: string[] };
            assert.equal(outcome.failed, undefined);
            const tables = outcome.tables ?? [];
            assert.ok(tables.includes("pi_entries"), tables.join(", "));
            assert.deepEqual(
                tables.filter(
                    (table) =>
                        !CORE_TABLE.test(table) && !table.startsWith("pi_"),
                ),
                [],
            );
        });
    }
});
