import { readFile } from "node:fs/promises";
import { setTimeout as sleep } from "node:timers/promises";
import {
    createModels,
    createProvider,
    type MutableModels,
} from "@earendil-works/pi-ai/models";
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

/** The model a faux script answers as, named `endpoint::id`. */
export const FAUX_MODEL = "faux::faux-1";

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
            /** Time the answer takes, which stopping the request does not cut short. */
            stallMs: Type.Optional(Type.Integer({ minimum: 0 })),
        },
        { additionalProperties: false },
    ),
]);

/** A script of answers given in a model's place, one per request in order: words, or words with tool calls (S14). */
export const Script = Type.Object(
    {
        responses: Type.Array(Step),
        /** Tokens a second an answer streams at; at once when absent. */
        tokensPerSecond: Type.Optional(Type.Number({ exclusiveMinimum: 0 })),
    },
    { additionalProperties: false },
);
export type Script = Static<typeof Script>;

const scripts = Compile(Script);

/** The script in the JSON file `path`. */
export async function readScript(path: string): Promise<Script> {
    const value: unknown = JSON.parse(await readFile(path, "utf8"));
    if (!scripts.Check(value)) {
        const [error] = scripts.Errors(value);
        const at = error?.instancePath || "it";
        throw new Error(
            `${path} is not a faux script: ${at} ${error?.message ?? "is not valid"}`,
        );
    }
    return value;
}

/** pi-ai models served only by the faux provider, which answers with `script`. */
export function scriptedModels(script: Script): MutableModels {
    const faux = createFauxCore(
        script.tokensPerSecond === undefined
            ? {}
            : { tokensPerSecond: script.tokensPerSecond },
    );
    faux.setResponses(script.responses.map(answer));
    const models = createModels();
    models.setProvider(
        createProvider({
            id: faux.provider,
            auth: {
                apiKey: { name: "Faux", resolve: async () => ({ auth: {} }) },
            },
            models: faux.models,
            api: { stream: faux.stream, streamSimple: faux.streamSimple },
        }),
    );
    return models;
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
    return async () => {
        if (step.stallMs !== undefined) await sleep(step.stallMs);
        return fauxAssistantMessage(content, {
            stopReason: calls.length > 0 ? "toolUse" : "stop",
        });
    };
}
