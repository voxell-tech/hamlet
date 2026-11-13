import assert from "node:assert/strict";
import { test } from "node:test";
import { fauxAssistantMessage, fauxToolCall } from "@earendil-works/pi-ai";
import {
    createRegistry,
    defineExtension,
    defineTool,
    hook,
    ToolTask,
} from "@earendil-works/pi-durable";
import type { RawAccess } from "../src/actor/db/mod.ts";
import {
    closeHarness,
    createPiRuntime,
    openOnWake,
    PI_RUNTIME,
    type PiContext,
    type PiOptions,
    piActor,
    withHarness,
} from "../src/pi/runtime.ts";
import { migratePiTables } from "../src/pi/storage.ts";
import { context, fauxModels, plainDatabase, scratchFile } from "./testing.ts";

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

/** The ids of the tool calls whose results the file holds, in order. */
async function toolResults(database: RawAccess): Promise<string[]> {
    const rows = await database.execute<{ call: string }>(
        "SELECT result.value ->> '$.toolCallId' AS call FROM pi_entries, json_each(record, '$.model') AS result WHERE record ->> '$.kind' = 'pi.tool-result' ORDER BY pi_entries.id",
    );
    return rows.map((row) => row.call);
}

test("each hook, tool and environment gets its actor's context, also after a reopen", {
    timeout: 30_000,
}, async (t) => {
    const database = plainDatabase(scratchFile(t));
    await migratePiTables(database);

    const seen: { by: "env" | "tool" | "beforeTool"; actor: PiContext }[] = [];
    const cutOff = Promise.withResolvers<void>();
    const tell = defineTool({
        name: "tell",
        description: "Send a letter.",
        parameters: { type: "object", properties: {} },
        replay: "safe",
        execute: async (_args, _api, toolContext) => {
            seen.push({ by: "tool", actor: piActor(toolContext) });
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
    const { faux, models, model } = fauxModels();
    faux.setResponses([
        fauxAssistantMessage([fauxToolCall("tell", {}, { id: "call-1" })], {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("done"),
    ]);
    const options: PiOptions = {
        registry,
        models,
        model,
        env: (_target, envContext) => {
            seen.push({ by: "env", actor: piActor(envContext) });
            return undefined;
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
    await closeHarness(first, { reason: "destroy" });
    assert.deepEqual(await toolResults(database), []);

    const second = generation(database);
    const secondHarness = await openOnWake(second, options);
    secondHarness.resume();
    const resumed = await secondHarness.submission(submission.id, context);
    assert.equal((await resumed?.wait(context))?.status, "done");

    assert.deepEqual(
        seen.map(({ by, actor }) => [by, actor === first ? 1 : 2]),
        [
            ["env", 1],
            ["beforeTool", 1],
            ["env", 1],
            ["tool", 1],
            ["env", 2],
            ["tool", 2],
            ["env", 2],
        ],
    );
    assert.deepEqual(await toolResults(database), ["call-1"]);
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
    ]);
    await database.close();
});

test("a call through withHarness gets its generation's context, not the call's", async (t) => {
    const database = plainDatabase(scratchFile(t));
    await migratePiTables(database);
    const { models } = fauxModels();
    const options: PiOptions = { registry: createRegistry(), models };
    const actor = generation(database);
    await openOnWake(actor, options);
    // An action's own context: another object over the same generation.
    const call = { ...actor } as PiContext;

    const seen = await withHarness(
        call,
        options,
        async (harness, callContext) => {
            await harness.root(callContext);
            return piActor(callContext);
        },
    );

    assert.equal(seen, actor);
    await closeHarness(actor, { reason: "destroy" });
    await database.close();
});
