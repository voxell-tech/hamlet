/**
 * The faux model the Node host's entries run in place of real ones. It
 * answers with a script's steps, one per request in order, and then the
 * lines below (`run:`, `tell`, `read` and the others), and the line `tools`
 * with the tools it is offered, one `<name>: <description>` a line.
 *
 * A script is JSON, after today's server's `--faux` file:
 *
 * ```json
 * {
 *   "models": ["faux-a", { "id": "faux-b", "input": 1, "output": 2, "window": 32000 }],
 *   "price": { "input": 3, "output": 15 },
 *   "responses": [
 *     "Plain words.",
 *     { "thinking": "...", "text": "...", "calls": [{ "name": "bash", "arguments": {} }] },
 *     { "text": "Slow.", "waitMs": 20000 },
 *     { "text": "Stalled.", "stallMs": 1000 },
 *     { "error": "The model failed." },
 *     { "text": "Big.", "usage": { "input": 100000, "output": 10 } }
 *   ]
 * }
 * ```
 *
 * A model is its id, or its id with its own `input` and `output` dollars per
 * million tokens and its context `window`; `price` is the dollars of a model
 * that names none. The agent runs the first model. A step's `waitMs` is the
 * time its answer takes, which stopping the request ends; its `stallMs` a
 * time stopping does not end; and its `usage` the tokens it says it used, in
 * place of the faux model's estimate. Each answer is priced from its model's
 * rates, as a real provider's is. A restarted host begins the script again.
 */
import { readFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import {
    type Api,
    type AssistantMessage,
    type AssistantMessageEventStream,
    calculateCost,
    createAssistantMessageEventStream,
    createFauxCore,
    createModels,
    createProvider,
    type FauxModelDefinition,
    type FauxResponseFactory,
    fauxAssistantMessage,
    fauxText,
    fauxThinking,
    fauxToolCall,
    getCurrentTools,
    type Message,
    type Model,
    type MutableModels,
    type ProviderStreams,
    type SimpleStreamOptions,
    type StreamFunction,
    type ToolCall,
} from "@earendil-works/pi-ai";

/** The model a script that names none runs, as today's does. */
const FAUX_MODEL = "faux-1";

/** The paths whose next edit is the last thing this process does. */
export const dying = new Set<string>();

interface Line {
    readonly form: RegExp;
    readonly tool: string;
    readonly args: (...groups: string[]) => ToolCall["arguments"];
}

const LINES: readonly Line[] = [
    {
        form: /^run: (.+)$/s,
        tool: "bash",
        args: (script) => ({ command: script }),
    },
    {
        form: /^tell ([1-9][0-9]*): (.+)$/s,
        tool: "tell",
        args: (n, text) => ({ to: `hire#${n}`, text }),
    },
    {
        form: /^wake ([0-9]+(?:\.[0-9]+)?): (.+)$/s,
        tool: "wake",
        args: (seconds, note) => ({ in_seconds: Number(seconds), note }),
    },
    {
        form: /^read (\S+)$/,
        tool: "read",
        args: (path) => ({ path }),
    },
    {
        form: /^write (\S+): (.*)$/s,
        tool: "write",
        args: (path, content) => ({ path, content }),
    },
    {
        form: /^object: (.+)$/s,
        tool: "object",
        args: (reason) => ({ reason }),
    },
    {
        form: /^(die in )?edit (\S+): (.+?) -> (.*)$/s,
        tool: "edit",
        args: (dies, path, oldText, newText) => {
            if (dies !== undefined) dying.add(path);
            return { path, edits: [{ oldText, newText }] };
        },
    },
];

function wordsOf(message: Message): string {
    const content = message.content;
    if (typeof content === "string") return content;
    return content
        .map((block) => (block.type === "text" ? block.text : ""))
        .join("");
}

/** Answers the newest message, past the prompt's updates before a request. */
const echo: FauxResponseFactory = (context): AssistantMessage => {
    const last = context.messages.findLast(
        (message) => message.role !== "system",
    );
    if (last === undefined) return fauxAssistantMessage("Heard nothing.");
    const words = wordsOf(last);
    if (last.role !== "user") return fauxAssistantMessage(`Done: ${words}`);
    if (words === "tools") {
        return fauxAssistantMessage(
            getCurrentTools(context.messages)
                .map((tool) => `${tool.name}: ${tool.description}`)
                .join("\n"),
        );
    }
    for (const line of LINES) {
        const found = line.form.exec(words);
        if (found === null) continue;
        return fauxAssistantMessage(
            fauxToolCall(line.tool, line.args(...found.slice(1))),
            { stopReason: "toolUse" },
        );
    }
    return fauxAssistantMessage(`Heard: ${words}`);
};

interface Tokens {
    readonly input: number;
    readonly output: number;
}

interface Step {
    readonly answer: FauxResponseFactory;
    readonly usage?: Tokens;
}

interface Script {
    readonly models?: readonly FauxModelDefinition[];
    readonly steps: Step[];
}

/** A script as its file holds it; the tests write it, so it is trusted. */
interface ScriptFile {
    readonly models?: readonly (
        | string
        | {
              readonly id: string;
              readonly input?: number;
              readonly output?: number;
              readonly window?: number;
          }
    )[];
    readonly price?: Tokens;
    readonly responses?: readonly (string | StepFile)[];
}

interface StepFile {
    readonly thinking?: string;
    readonly text?: string;
    readonly calls?: readonly {
        readonly name: string;
        readonly arguments: ToolCall["arguments"];
    }[];
    readonly error?: string;
    readonly waitMs?: number;
    readonly stallMs?: number;
    readonly usage?: Tokens;
}

function scriptOf(path: string): Script {
    const file = JSON.parse(readFileSync(path, "utf8")) as ScriptFile;
    const price = file.price ?? { input: 0, output: 0 };
    const named =
        file.models ?? (file.price === undefined ? undefined : [FAUX_MODEL]);
    return {
        ...(named === undefined
            ? {}
            : {
                  models: named.map((entry) => {
                      const model =
                          typeof entry === "string" ? { id: entry } : entry;
                      return {
                          id: model.id,
                          cost: {
                              input: model.input ?? price.input,
                              output: model.output ?? price.output,
                              cacheRead: 0,
                              cacheWrite: 0,
                          },
                          ...(model.window === undefined
                              ? {}
                              : { contextWindow: model.window }),
                      };
                  }),
              }),
        steps: (file.responses ?? []).map((step) =>
            typeof step === "string"
                ? { answer: () => fauxAssistantMessage(step) }
                : stepOf(step),
        ),
    };
}

function stepOf(step: StepFile): Step {
    const { thinking, text, calls = [], error, waitMs, stallMs, usage } = step;
    const answer: FauxResponseFactory = async (_context, options) => {
        const signal = options?.signal;
        if (stallMs !== undefined) await sleep(stallMs);
        if (waitMs !== undefined)
            await sleep(waitMs, undefined, { signal }).catch(() => {});
        if (signal?.aborted)
            return fauxAssistantMessage("", { stopReason: "aborted" });
        const content = [
            ...(thinking === undefined ? [] : [fauxThinking(thinking)]),
            ...(text === undefined ? [] : [fauxText(text)]),
            ...calls.map((call) => fauxToolCall(call.name, call.arguments)),
        ];
        if (error !== undefined)
            return fauxAssistantMessage(content, {
                stopReason: "error",
                errorMessage: error,
            });
        return fauxAssistantMessage(content, {
            stopReason: calls.length > 0 ? "toolUse" : "stop",
        });
    };
    return usage === undefined ? { answer } : { answer, usage };
}

function measured(
    source: AssistantMessageEventStream,
    model: Model<Api>,
    usage: Tokens | undefined,
): AssistantMessageEventStream {
    const out = createAssistantMessageEventStream();
    void (async () => {
        for await (const event of source) {
            if (event.type === "done") {
                const used = event.message.usage;
                if (usage !== undefined) {
                    used.input = usage.input;
                    used.output = usage.output;
                    used.cacheRead = 0;
                    used.cacheWrite = 0;
                    used.totalTokens = usage.input + usage.output;
                }
                used.cost = calculateCost(model, used);
            }
            out.push(event);
        }
        out.end(await source.result());
    })();
    return out;
}

const SIGNED_IN = {
    access: 'made-up-oauth-access-"demo',
    refresh: 'made-up-oauth-refresh-"demo',
};

function signInProviders(api: ProviderStreams, fauxApi: string) {
    const modelsOf = (provider: string) =>
        createFauxCore({ provider, api: fauxApi, models: [{ id: "large" }] })
            .models;
    const loopback = createProvider({
        id: "loopback",
        models: modelsOf("loopback"),
        api,
        auth: {
            apiKey: {
                name: "loopback key",
                login: async (interaction) => ({
                    type: "api_key",
                    key: await interaction.prompt({
                        type: "secret",
                        message: "Paste your key.",
                    }),
                }),
                resolve: async ({ credential }) =>
                    credential?.key === undefined
                        ? undefined
                        : { auth: { apiKey: credential.key } },
            },
        },
    });
    const chatgpt = createProvider({
        id: "chatgpt",
        models: modelsOf("chatgpt"),
        api,
        auth: {
            oauth: {
                name: "loopback subscription",
                login: async (interaction, options) => {
                    interaction.notify({
                        type: "auth_url",
                        url: "https://example.invalid/oauth",
                        instructions: "Offline sign-in",
                    });
                    interaction.notify({
                        type: "device_code",
                        verificationUri: "https://example.invalid/device",
                        userCode: "SAFE-CODE",
                    });
                    interaction.notify({
                        type: "progress",
                        message: "Waiting for the offline code",
                    });
                    await interaction.prompt({
                        type: "manual_code",
                        message: "Paste the code.",
                    });
                    return {
                        type: "oauth",
                        ...SIGNED_IN,
                        expires: Date.now() + 3_600_000,
                        clientId: options?.getDeviceId?.(),
                        agent: options?.agentName,
                        scopes: ["offline", "profile"],
                    };
                },
                refresh: async (credential) => ({
                    ...credential,
                    expires: Date.now() + 3_600_000,
                }),
                toAuth: async (credential) => ({ apiKey: credential.access }),
            },
        },
    });
    return [loopback, chatgpt];
}

export interface FauxModels {
    readonly models: MutableModels;
    /** The model the agent runs, as `provider/modelId`. */
    readonly model: string;
}

/**
 * The faux provider answering from the script at `path`, or from the lines
 * alone without one; with `signIn`, also two providers on loopback to sign
 * in to, which answer as it does.
 */
export function fauxModels(
    path: string | undefined,
    signIn: boolean,
): FauxModels {
    const script: Script = path === undefined ? { steps: [] } : scriptOf(path);
    const core = createFauxCore(
        script.models === undefined ? {} : { models: [...script.models] },
    );
    // Each request takes its step as it is made, so the answer and its usage
    // stay together when requests overlap.
    const stream: StreamFunction<string, SimpleStreamOptions> = (
        model,
        context,
        options,
    ) => {
        const step = script.steps.shift() ?? { answer: echo };
        core.setResponses([step.answer]);
        return measured(
            core.stream(model, context, options),
            model,
            step.usage,
        );
    };
    const api: ProviderStreams = { stream, streamSimple: stream };
    const models = createModels();
    models.setProvider(
        createProvider({
            id: core.provider,
            auth: {
                apiKey: { name: "Faux", resolve: async () => ({ auth: {} }) },
            },
            models: core.models,
            api,
        }),
    );
    if (signIn) {
        for (const provider of signInProviders(api, core.api)) {
            models.setProvider(provider);
        }
    }
    const { provider, id } = core.getModel();
    return { models, model: `${provider}/${id}` };
}
