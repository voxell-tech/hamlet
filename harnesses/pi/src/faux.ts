import { access, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import {
    calculateCost,
    createAssistantMessageEventStream,
    type ProviderStreams,
    type SimpleStreamOptions,
    type StreamFunction,
} from "@earendil-works/pi-ai";
import { createProvider, type Provider } from "@earendil-works/pi-ai/models";
import {
    createFauxCore,
    type FauxContentBlock,
    type FauxResponseFactory,
    fauxAssistantMessage,
    fauxText,
    fauxThinking,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { type Static, Type } from "typebox";
import { Compile } from "typebox/compile";

type ToolArguments = Parameters<typeof fauxToolCall>[1];

/** The provider and model a faux script answers as. */
export const FAUX = { provider: "faux", id: "faux-1" } as const;

const Step = Type.Union([
    Type.String(),
    Type.Object(
        {
            text: Type.Optional(Type.String()),
            thinking: Type.Optional(Type.String()),
            calls: Type.Optional(
                Type.Array(
                    Type.Object(
                        {
                            name: Type.String(),
                            arguments: Type.Record(
                                Type.String(),
                                Type.Unknown(),
                            ),
                        },
                        { additionalProperties: false },
                    ),
                ),
            ),
            /** The model's failure, answered in place of words and calls. */
            error: Type.Optional(Type.String()),
            /** Time the answer takes, which stopping the request does not cut short. */
            stallMs: Type.Optional(Type.Integer({ minimum: 0 })),
            /** A file the answer waits for, relative to the script's folder; stopping the request ends the wait. */
            holdUntil: Type.Optional(Type.String({ minLength: 1 })),
        },
        { additionalProperties: false },
    ),
]);

const Steps = Type.Array(Step);

/**
 * Answers given in a model's place, one per request in order. A conversation of the agent `<hamlet>:<number>` takes
 * its answers from `agents`, a one-off conversation from `errands`, and either takes from `responses` only when its own
 * list is absent. The format is the acceptance suite's.
 */
export const Script = Type.Object(
    {
        responses: Type.Optional(Steps),
        agents: Type.Optional(
            Type.Record(Type.String({ pattern: "^[^:]+:[0-9]+$" }), Steps, {
                additionalProperties: false,
            }),
        ),
        errands: Type.Optional(Steps),
        /** Dollars per million tokens the faux model charges; the catalogue's price when absent. */
        price: Type.Optional(
            Type.Object(
                {
                    input: Type.Number({ minimum: 0 }),
                    output: Type.Number({ minimum: 0 }),
                },
                { additionalProperties: false },
            ),
        ),
        /** Tokens a second an answer streams at; at once when absent. */
        tokensPerSecond: Type.Optional(Type.Number({ exclusiveMinimum: 0 })),
    },
    { additionalProperties: false },
);
export type Script = Static<typeof Script>;

const scripts = Compile(Script);

/** The script in the JSON file `path`, with its held files resolved from the script's folder. */
export async function readScript(path: string): Promise<Script> {
    const value: unknown = JSON.parse(await readFile(path, "utf8"));
    if (!scripts.Check(value)) {
        const [error] = scripts.Errors(value);
        const at = error?.instancePath || "it";
        throw new Error(
            `${path} is not a faux script: ${at} ${error?.message ?? "is not valid"}`,
        );
    }
    const folder = dirname(resolve(path));
    const held = (steps: Static<typeof Steps>) =>
        steps.map((step) =>
            typeof step === "string" || step.holdUntil === undefined
                ? step
                : { ...step, holdUntil: resolve(folder, step.holdUntil) },
        );
    return {
        ...value,
        ...(value.responses === undefined
            ? {}
            : { responses: held(value.responses) }),
        ...(value.agents === undefined
            ? {}
            : {
                  agents: Object.fromEntries(
                      Object.entries(value.agents).map(([key, steps]) => [
                          key,
                          held(steps),
                      ]),
                  ),
              }),
        ...(value.errands === undefined
            ? {}
            : { errands: held(value.errands) }),
    };
}

/**
 * The faux provider answering with `script`. `caller` names the list a request takes from, `errands` or an agent's
 * `<hamlet>:<number>`, by the request's options. `guard` wraps the provider's streams.
 */
export function scriptedProvider(
    script: Script,
    caller: (options: SimpleStreamOptions | undefined) => string,
    guard: (streams: ProviderStreams) => ProviderStreams,
): Provider {
    const faux = createFauxCore({
        ...(script.tokensPerSecond === undefined
            ? {}
            : { tokensPerSecond: script.tokensPerSecond }),
        ...(script.price === undefined
            ? {}
            : {
                  models: [
                      {
                          id: FAUX.id,
                          cost: {
                              ...script.price,
                              cacheRead: 0,
                              cacheWrite: 0,
                          },
                      },
                  ],
              }),
    });
    const shared = (script.responses ?? []).map(answer);
    const queues = new Map<string, FauxResponseFactory[]>([
        ...Object.entries(script.agents ?? {}).map(
            ([key, steps]): [string, FauxResponseFactory[]] => [
                key,
                steps.map(answer),
            ],
        ),
        ...(script.errands === undefined
            ? []
            : [["errands", script.errands.map(answer)] as const]),
    ]);
    const exhausted: FauxResponseFactory = () =>
        fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: "No more faux responses queued",
        });
    // Set just before the core reads it, after the server's allowing, so a concurrent call cannot take another's step.
    const answered =
        (
            step: FauxResponseFactory,
        ): StreamFunction<string, SimpleStreamOptions> =>
        (model, context, options) => {
            faux.setResponses([step]);
            return faux.stream(model, context, options);
        };
    // A call takes its step as it is made, so concurrent calls take successive steps in the order they were made.
    const stream: StreamFunction<string, SimpleStreamOptions> = (
        model,
        context,
        options,
    ) => {
        const step = (queues.get(caller(options)) ?? shared).shift();
        const once = priced(answered(step ?? exhausted));
        return guard({ stream: once, streamSimple: once }).stream(
            model,
            context,
            options,
        );
    };
    return createProvider({
        id: faux.provider,
        auth: {
            apiKey: { name: "Faux", resolve: async () => ({ auth: {} }) },
        },
        models: faux.models,
        api: { stream, streamSimple: stream },
    });
}

/** Prices each answer from its model's rates, as a real provider does. */
function priced(
    stream: StreamFunction<string, SimpleStreamOptions>,
): StreamFunction<string, SimpleStreamOptions> {
    return (model, context, options) => {
        const source = stream(model, context, options);
        const out = createAssistantMessageEventStream();
        void (async () => {
            for await (const event of source) {
                if (event.type === "done") {
                    event.message.usage.cost = calculateCost(
                        model,
                        event.message.usage,
                    );
                }
                out.push(event);
            }
            out.end(await source.result());
        })();
        return out;
    };
}

/** The answer of one step, made when it is asked for so it carries that time. */
function answer(step: Static<typeof Step>): FauxResponseFactory {
    if (typeof step === "string") return () => fauxAssistantMessage(step);
    const calls = step.calls ?? [];
    const content: FauxContentBlock[] = [
        ...(step.thinking === undefined ? [] : [fauxThinking(step.thinking)]),
        ...(step.text === undefined ? [] : [fauxText(step.text)]),
        ...calls.map((call) =>
            // Parsed from JSON, so JSON throughout.
            fauxToolCall(call.name, call.arguments as ToolArguments),
        ),
    ];
    return async (_prompt, options) => {
        if (step.stallMs !== undefined) await sleep(step.stallMs);
        if (step.holdUntil !== undefined)
            await held(step.holdUntil, options?.signal);
        if (step.error !== undefined)
            return fauxAssistantMessage(content, {
                stopReason: "error",
                errorMessage: step.error,
            });
        return fauxAssistantMessage(content, {
            stopReason: calls.length > 0 ? "toolUse" : "stop",
        });
    };
}

/** Waits until `file` exists or `signal` aborts. */
async function held(file: string, signal: AbortSignal | undefined) {
    while (!signal?.aborted) {
        try {
            await access(file);
            return;
        } catch {
            await sleep(20);
        }
    }
}
