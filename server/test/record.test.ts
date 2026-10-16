import { Type } from "@earendil-works/pi-ai";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import {
    defineDoc,
    defineExtension,
    defineTool,
    type StorageWrite,
} from "@earendil-works/pi-durable";
import { describe, expect, it } from "vitest";
import { defineRecordTool, HOME, RecordDoc } from "../src/record.ts";
import {
    context,
    crashing,
    fauxModels,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

/** The commit that makes a record tool's change, with its marker. */
const change = (writes: readonly StorageWrite[]) =>
    writes.some(
        (write) =>
            write.type === "document.create" &&
            write.record.kind === "hamlet.record-call",
    );

/** The commit that appends a tool's result. */
const result = (writes: readonly StorageWrite[]) =>
    writes.some(
        (write) =>
            write.type === "entry" && write.value.kind === "pi.tool-result",
    );

describe("a record tool", () => {
    it.each([
        ["before its change", change],
        ["between its change and its result", result],
    ])(
        "cut off %s makes the change once and gives its result",
        async (_when, crashes) => {
            const dir = scratch();
            const { models, faux } = fauxModels();
            faux.setResponses([
                fauxAssistantMessage(
                    fauxToolCall("create_agent", {
                        name: "tester",
                        purpose: "Runs the tests.",
                        reason: "Nobody runs them.",
                    }),
                    { stopReason: "toolUse" },
                ),
                fauxAssistantMessage("Done."),
            ]);
            const before = await testHome(dir, {
                models,
                openStorage: crashing(crashes),
            });
            const submission = await say(before, "Hire a tester.");
            expect(await before.harness.closed).toMatchObject({
                reason: "failed",
            });

            const after = await testHome(dir, { models });
            const settled = await (
                await after.harness.submission(submission.id, context)
            )?.wait(context);

            expect(settled?.status).toBe("done");
            const record = await after.harness.snapshot(
                RecordDoc,
                HOME,
                context,
            );
            expect(record?.agents.map((agent) => agent.name)).toEqual([
                "founder",
                "tester",
            ]);
            const results = (await transcript(await mayorChat(after))).filter(
                (entry) => entry.kind === "pi.tool-result",
            );
            expect(results).toHaveLength(1);
            expect(JSON.stringify(results[0]?.model)).toContain("tester");
            expect(JSON.stringify(results[0]?.model)).not.toContain(
                "interrupted",
            );
        },
    );
});

it("replays the pre-upgrade record-call memo without re-running its change", async () => {
    const legacyMemo = defineDoc<{ result: string | null }>({
        kind: "hamlet.record-call",
        version: 1,
        scope: "task",
        initial: () => ({ result: null }),
    });
    const dir = scratch();
    const { models, faux } = fauxModels();
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("legacy_record", {}), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage("Done."),
    ]);
    const before = await testHome(dir, {
        models,
        openStorage: crashing(result),
        parts: [
            () => ({
                extension: defineExtension({
                    name: "legacy.memo",
                    tools: [
                        defineTool({
                            name: "legacy_record",
                            description: "Legacy record writer",
                            parameters: Type.Object({}),
                            replay: "safe",
                            execute: async (_args, api, context) => {
                                // This writer intentionally uses the pre-upgrade document shape and no replay helper.
                                await api.commit(async (tx) => {
                                    (
                                        await tx.doc(legacyMemo, api.taskId)
                                    ).result = "The old answer.";
                                }, context);
                                return {
                                    output: [
                                        {
                                            type: "text",
                                            text: "The old answer.",
                                        },
                                    ],
                                };
                            },
                        }),
                    ],
                }),
            }),
        ],
    });
    const submission = await say(before, "Make the old change.");
    await before.harness.closed;
    let changes = 0;
    const after = await testHome(dir, {
        models,
        parts: [
            () => ({
                extension: defineExtension({
                    name: "legacy.memo",
                    tools: [
                        defineRecordTool({
                            name: "legacy_record",
                            description: "Legacy record writer",
                            parameters: Type.Object({}),
                            change: async () => {
                                changes++;
                                return "A duplicate change.";
                            },
                        }),
                    ],
                }),
            }),
        ],
    });
    expect(
        (
            await (
                await after.harness.submission(submission.id, context)
            )?.wait(context)
        )?.status,
    ).toBe("done");
    expect(changes).toBe(0);
    const results = (await transcript(await mayorChat(after))).filter(
        (e) => e.kind === "pi.tool-result",
    );
    expect(JSON.stringify(results)).toContain("The old answer.");
    expect(JSON.stringify(results)).not.toContain("duplicate");
});

it("preserves the model's error diagnostic when removal returns a refusal", async () => {
    const { models, faux } = fauxModels();
    faux.setResponses([
        fauxAssistantMessage(
            fauxToolCall("remove_agent", {
                name: "nobody",
                reason: "Finished",
            }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("Done."),
    ]);
    const home = await testHome(scratch(), { models });
    await (await say(home, "Remove nobody.")).wait(context);
    const result = (await transcript(await mayorChat(home))).find(
        (entry) => entry.kind === "pi.tool-result",
    );
    expect(result?.model?.[0]).toMatchObject({
        isError: true,
        content: [
            {
                type: "text",
                text: "<harness>\n[error] no agent named `nobody`\n</harness>",
            },
        ],
    });
});

it("offers create_agent as replay safe", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const tools = (await (await mayorChat(home)).agent(context)).tools;
    expect(tools.find((tool) => tool.name === "create_agent")?.replay).toBe(
        "safe",
    );
});
