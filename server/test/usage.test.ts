import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import { createModels, createProvider } from "@earendil-works/pi-ai/models";
import {
    createFauxCore,
    fauxAssistantMessage,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { chat, request, say, served } from "./connection-support.ts";

it.each([
    {
        input: 0,
        output: 0,
        ms: 42_000,
        cost: 0,
        summary: "Tokens not reported, 42s model time",
    },
    {
        input: 17_000,
        output: 1240,
        ms: 840_000,
        cost: 0,
        summary: "18.2k tok, 14m model time",
    },
    {
        input: 17_000,
        output: 1240,
        ms: 840_000,
        cost: 0.4249,
        summary: "18.2k tok, $0.42",
    },
    {
        input: 1_200_000,
        output: 0,
        ms: 840_000,
        cost: 0,
        summary: "1.20M tok, 14m model time",
    },
])(
    "shows honest conversation usage: $summary",
    async ({ input, output, ms, cost, summary }) => {
        const faux = createFauxCore({});
        faux.setResponses([fauxAssistantMessage("Done.")]);
        const stream: typeof faux.stream = (model, inputContext, options) => {
            const source = faux.stream(model, inputContext, options);
            const out = createAssistantMessageEventStream();
            void (async () => {
                for await (const event of source) {
                    if (event.type === "done") {
                        event.message.usage = {
                            input,
                            output,
                            cacheRead: 0,
                            cacheWrite: 0,
                            totalTokens: input + output,
                            cost: {
                                input: 0,
                                output: 0,
                                cacheRead: 0,
                                cacheWrite: 0,
                                total: cost,
                            },
                        };
                        event.message.durationMs = ms;
                    }
                    out.push(event);
                }
                out.end(await source.result());
            })();
            return out;
        };
        const models = createModels();
        models.setProvider(
            createProvider({
                id: faux.provider,
                models: faux.models,
                auth: {
                    apiKey: {
                        name: "Faux",
                        resolve: async () => ({ auth: {} }),
                    },
                },
                api: { stream, streamSimple: stream },
            }),
        );
        const run = await served({
            models,
            env: { HAMLET_REFLECT_EVERY: "0" },
        });
        expect(
            await request(run.connection, run.messages, {
                type: "open",
                id: crypto.randomUUID(),
                chat,
            }),
        ).toMatchObject({
            type: "transcript",
            usage: { summary: "No model calls yet" },
        });
        say(run.connection, "Hello.");
        await vi.waitFor(() =>
            expect(
                run.messages
                    .flatMap((message) =>
                        message.type === "turn" ? message.ops : [],
                    )
                    .filter((op) => op.op === "usage")
                    .at(-1),
            ).toMatchObject({
                op: "usage",
                usage: {
                    calls: 1,
                    input,
                    output,
                    ms,
                    micros: Math.round(cost * 1_000_000),
                    summary,
                },
            }),
        );
        expect(
            await request(run.connection, run.messages, {
                type: "open",
                id: crypto.randomUUID(),
                chat,
            }),
        ).toMatchObject({ type: "transcript", usage: { summary } });
    },
);
