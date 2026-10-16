import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import {
    defineExtension,
    GenerationTask,
    type HookApi,
    hook,
    LiveDoc,
    ToolTask,
} from "@earendil-works/pi-durable";
import { describe, expect, it } from "vitest";
import { ChiefEntry, type Turn, turnOf } from "../src/chats.ts";
import type { Part } from "../src/home.ts";
import {
    CHIEF,
    context,
    fauxModels,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

const round = (command: string) =>
    fauxAssistantMessage(fauxToolCall("bash", { command }), {
        stopReason: "toolUse",
    });

describe("a turn", () => {
    it("is the same in every hook of a run, while pi hands the run to a new generation each round", async () => {
        const seen: { hook: string; turn?: Turn; run?: number }[] = [];
        const look: Part = (home) => {
            const note = async (name: string, api: HookApi) => {
                const live = await api.snapshot(
                    LiveDoc,
                    api.conversationId,
                    context,
                );
                const turn = await turnOf(home.harness, api, context);
                seen.push({
                    hook: name,
                    ...(turn && { turn }),
                    ...(live?.run && { run: live.run.taskId }),
                });
            };
            return {
                extension: defineExtension({
                    name: "test.turn",
                    hooks: [
                        hook(GenerationTask, {
                            beforeRequest: (_request, api) =>
                                note("request", api).then(() => undefined),
                        }),
                        hook(ToolTask, {
                            beforeTool: (_call, api) =>
                                note("tool", api).then(() => undefined),
                        }),
                    ],
                }),
            };
        };
        const { models, faux } = fauxModels();
        const home = await testHome(scratch(), { models, parts: [look] });
        faux.setResponses([
            round("true"),
            round("true"),
            round("true"),
            fauxAssistantMessage("Done."),
        ]);

        const submission = await say(home, "Run it three times.");
        expect((await submission.wait(context)).status).toBe("done");

        const [line] = await transcript(await mayorChat(home));
        if (!ChiefEntry.is(line)) throw new Error("no chief's line");
        expect(seen.map((each) => each.hook)).toEqual([
            "request",
            "tool",
            "request",
            "tool",
            "request",
            "tool",
            "request",
        ]);
        for (const { turn } of seen) {
            expect(turn).toEqual({
                input: submission.id,
                kind: "hamlet.chief",
                task: line.data.task,
                chief: CHIEF,
                from: null,
            });
        }
        const runs = seen
            .filter((each) => each.hook === "request")
            .map((each) => each.run);
        expect(new Set(runs).size).toBe(4);
    });

    it("runs the tool calls of one round one at a time, in the model's order", async () => {
        const dir = scratch();
        const { models, faux } = fauxModels();
        const home = await testHome(dir, { models });
        faux.setResponses([
            fauxAssistantMessage(
                [
                    fauxToolCall("bash", {
                        command: "sleep 0.3; echo first >> order.txt",
                    }),
                    fauxToolCall("bash", {
                        command: "echo second >> order.txt",
                    }),
                ],
                { stopReason: "toolUse" },
            ),
            fauxAssistantMessage("Done."),
        ]);

        expect((await (await say(home, "Go.")).wait(context)).status).toBe(
            "done",
        );

        expect(readFileSync(join(dir, "houses/home/0/order.txt"), "utf8")).toBe(
            "first\nsecond\n",
        );
    });
});
