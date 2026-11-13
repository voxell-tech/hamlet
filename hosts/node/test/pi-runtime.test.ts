import assert from "node:assert/strict";
import { test } from "node:test";
import {
    createModels,
    fauxAssistantMessage,
    fauxProvider,
    fauxToolCall,
} from "@earendil-works/pi-ai";
import {
    createRegistry,
    defineExtension,
    defineTool,
    hook,
    ToolTask,
} from "@earendil-works/pi-durable";
import type { RawAccess } from "../src/actor/db/mod.ts";
import { type AgentDatabase, agentDatabase } from "../src/agent/database.ts";
import {
    closeHarness,
    createPiRuntime,
    openOnWake,
    PI_RUNTIME,
    type PiContext,
    type PiOptions,
    piActor,
} from "../src/pi/runtime.ts";
import { migratePiTables } from "../src/pi/storage.ts";
import {
    context,
    type Letter,
    plainDatabase,
    scratchFile,
    stored,
    writeLetter,
} from "./testing.ts";

/**
 * The members of an actor's context the pi wrapper uses, over a plain SQLite
 * file: one actor generation, with no actor runtime under it.
 */
function generation(database: RawAccess): PiContext {
    return {
        actorId: "agent-1",
        db: database,
        vars: { [PI_RUNTIME]: createPiRuntime() },
        conns: new Map(),
        log: { warn: () => {}, error: () => {} },
        keepAwake: (promise: Promise<unknown>) => promise,
        schedule: { at: async () => {} },
        abortSignal: new AbortController().signal,
    } as unknown as PiContext;
}

/** Every table in the file, by name. */
async function tables(database: RawAccess): Promise<string[]> {
    const rows = await database.execute<{ name: string }>(
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    );
    return rows.map((row) => row.name);
}

test("each hook and tool gets its actor's context, and the wrapper writes through the database layer", {
    timeout: 30_000,
}, async (t) => {
    const database = plainDatabase(scratchFile(t));
    await database.execute(
        "CREATE TABLE stand_in_sends (call TEXT PRIMARY KEY, recipient TEXT NOT NULL, body TEXT NOT NULL)",
    );
    await migratePiTables(database);

    const layers = new Map<PiContext, AgentDatabase<Letter>>();
    const layerOf = (actor: PiContext) => {
        const layer = layers.get(actor);
        if (layer === undefined) throw new Error("no layer for this actor");
        return layer;
    };
    const seen: { by: "tool" | "beforeTool"; actor: PiContext }[] = [];
    const cutOff = Promise.withResolvers<void>();
    const tell = defineTool({
        name: "tell",
        description: "Send a letter.",
        parameters: { type: "object", properties: {} },
        replay: "safe",
        execute: async (_args, api, toolContext) => {
            const actor = piActor(toolContext);
            seen.push({ by: "tool", actor });
            layerOf(actor).buffer(api, [
                { call: api.callId, to: "lead", body: "hello" },
            ]);
            if (seen.filter((sighting) => sighting.by === "tool").length > 1) {
                return { output: [{ type: "text", text: "sent" }] };
            }
            // The first run is cut off: it waits until its generation closes.
            cutOff.resolve();
            const { promise, reject } = Promise.withResolvers<never>();
            toolContext.abortSignal?.addEventListener("abort", () =>
                reject(new Error("cut off")),
            );
            return promise;
        },
    });
    const registry = createRegistry();
    registry.install(
        defineExtension({
            name: "letters",
            tools: [tell],
            hooks: [
                hook(ToolTask, {
                    beforeTool: (_call, _api, hookContext) => {
                        seen.push({
                            by: "beforeTool",
                            actor: piActor(hookContext),
                        });
                        return undefined;
                    },
                }),
            ],
        }),
    );
    const faux = fauxProvider();
    const models = createModels();
    models.setProvider(faux.provider);
    const model = faux.getModel();
    faux.setResponses([
        fauxAssistantMessage([fauxToolCall("tell", {}, { id: "call-1" })], {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("done"),
    ]);
    const options: PiOptions = {
        registry,
        models,
        model: `${model.provider}/${model.id}`,
        databaseLayer: (actor, base) => {
            const layer = agentDatabase(base, writeLetter);
            layers.set(actor, layer);
            return layer;
        },
    };

    const first = generation(database);
    const firstHarness = await openOnWake(first, options);
    firstHarness.resume();
    const root = await firstHarness.root(context);
    const submission = await root.submit(
        { type: "input", content: "Write to the lead.", requestId: "r1" },
        context,
    );
    await Promise.race([
        cutOff.promise,
        submission.wait(context).then((settled) => {
            throw new Error(
                `the run ended before its tool was cut off: ${settled.status}`,
            );
        }),
    ]);
    assert.equal(layerOf(first).buffered, 1);
    await closeHarness(first, { reason: "destroy" });
    assert.equal(layerOf(first).buffered, 0);
    assert.deepEqual((await stored(database)).letters, []);

    const second = generation(database);
    const secondHarness = await openOnWake(second, options);
    secondHarness.resume();
    const resumed = await secondHarness.submission(submission.id, context);
    assert.equal((await resumed?.wait(context))?.status, "done");

    assert.deepEqual(
        seen.map(({ by, actor }) => [by, actor === first ? 1 : 2]),
        [
            ["beforeTool", 1],
            ["tool", 1],
            ["tool", 2],
        ],
    );
    assert.deepEqual(await stored(database), {
        results: [{ call: "call-1", isError: false }],
        letters: [{ call: "call-1", to: "lead", body: "hello" }],
    });
    assert.equal(layerOf(second).buffered, 0);
    await closeHarness(second, { reason: "destroy" });

    assert.deepEqual(await tables(database), [
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
        "stand_in_sends",
    ]);
    await database.close();
});
