/**
 * The faux model the Node host's entries run in place of real ones. It
 * answers with a script's steps, one per request in order, and then the
 * lines below (`run:`, `tell`, `read` and the others), and the line `tools`
 * with the tools it is offered, one `<name>: <description>` a line.
 *
 * A script is JSON, in the server's `--faux` format:
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
 *     { "text": "Big.", "usage": { "input": 100000, "output": 10 } },
 *     { "text": "Late.", "holdUntil": "go" },
 *     { "calls": [{ "name": "tell", "arguments": { "to": "{{hamlet}}" } }], "fill": "names.json" }
 *   ],
 *   "agents": { "2": ["For agent 2."] },
 *   "kinds": { "worker": ["For any worker."] },
 *   "tokensPerSecond": 100
 * }
 * ```
 *
 * A model is its id, or its id with its own `input` and `output` dollars per
 * million tokens and its context `window`; `price` is the dollars of a model
 * that names none. The agent runs the first model. A step's `waitMs` is the
 * time its answer takes, which stopping the request ends; its `stallMs` a
 * time stopping does not end; and its `usage` the tokens it says it used, in
 * place of the faux model's estimate. A step's `holdUntil` is a file, from the
 * script's folder, its answer waits for until stopped; its `fill` a JSON file
 * of words, from the script's folder, for each `{{name}}` in its calls'
 * arguments. The agent numbered `<n>` takes its answers from `agents`, else
 * an agent of a kind such as `worker`, `errand` or `dream` (a dream's
 * errand) from `kinds`, and either takes from `responses` only when it has
 * no list of its own. Each answer is priced from its model's rates, as a
 * real provider's is. A restarted host begins the script again.
 */
import { AsyncLocalStorage } from "node:async_hooks";
import { readFileSync } from "node:fs";
import { access, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
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
    type FauxContentBlock,
    type FauxModelDefinition,
    type FauxResponseFactory,
    fauxAssistantMessage,
    fauxText,
    fauxThinking,
    fauxToolCall,
    getCurrentTools,
    type Message,
    type Model,
    type Models,
    type MutableModels,
    type ProviderStreams,
    type SimpleStreamOptions,
    type StreamFunction,
    type ToolCall,
} from "@earendil-works/pi-ai";

/** The model a script that names none runs. */
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
    readonly tokensPerSecond?: number;
    readonly steps: Step[];
    /** Each agent's and each kind's own steps, as `agent:<n>` and `kind:<kind>`. */
    readonly lists: Map<string, Step[]>;
}

/** The agent a model request is made for, and its kind. */
const caller = new AsyncLocalStorage<{ agent: number; kind: string }>();

/** `models`, whose faux requests take the steps of the agent `agent` of `kind`. */
export function answeringAs(
    agent: number,
    kind: string,
    models: Models,
): Models {
    const as = { agent, kind };
    return new Proxy(models, {
        get(target, key, receiver) {
            const value: unknown = Reflect.get(target, key, receiver);
            if (typeof value !== "function") return value;
            const bound = value.bind(target);
            return key === "streamSimple" || key === "stream"
                ? (...args: unknown[]) => caller.run(as, () => bound(...args))
                : bound;
        },
    });
}

/** The steps the request being made takes from. */
function stepsOf(script: Script): Step[] {
    const asked = caller.getStore();
    if (asked === undefined) return script.steps;
    return (
        script.lists.get(`agent:${asked.agent}`) ??
        script.lists.get(`kind:${asked.kind}`) ??
        script.steps
    );
}

/** The names and their words in the JSON file `file`. */
async function namesIn(file: string): Promise<Record<string, string>> {
    const value: unknown = JSON.parse(await readFile(file, "utf8"));
    if (
        typeof value !== "object" ||
        value === null ||
        Object.values(value).some((v) => typeof v !== "string")
    )
        throw new Error(`${file} is not an object of words`);
    return value as Record<string, string>;
}

/** `value` with each `{{name}}` in its strings replaced by the words of `name`; a name with none stays. */
function filled(value: unknown, words: Record<string, string>): unknown {
    if (typeof value === "string")
        return value.replace(
            /\{\{([^{}]+)\}\}/g,
            (whole, name: string) => words[name] ?? whole,
        );
    if (Array.isArray(value)) return value.map((item) => filled(item, words));
    if (typeof value === "object" && value !== null)
        return Object.fromEntries(
            Object.entries(value).map(([key, item]) => [
                key,
                filled(item, words),
            ]),
        );
    return value;
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
    readonly agents?: Readonly<Record<string, readonly (string | StepFile)[]>>;
    readonly kinds?: Readonly<Record<string, readonly (string | StepFile)[]>>;
    readonly tokensPerSecond?: number;
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
    readonly holdUntil?: string;
    readonly fill?: string;
    readonly usage?: Tokens;
}

function scriptOf(path: string): Script {
    const file = JSON.parse(readFileSync(path, "utf8")) as ScriptFile;
    const folder = dirname(resolve(path));
    const steps = (listed: readonly (string | StepFile)[]): Step[] =>
        listed.map((step) =>
            typeof step === "string"
                ? { answer: () => fauxAssistantMessage(step) }
                : stepOf(step, folder),
        );
    const lists = (
        prefix: string,
        named: Readonly<Record<string, readonly (string | StepFile)[]>> = {},
    ): [string, Step[]][] =>
        Object.entries(named).map(([key, listed]) => [
            `${prefix}:${key}`,
            steps(listed),
        ]);
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
        ...(file.tokensPerSecond === undefined
            ? {}
            : { tokensPerSecond: file.tokensPerSecond }),
        steps: steps(file.responses ?? []),
        lists: new Map([
            ...lists("agent", file.agents),
            ...lists("kind", file.kinds),
        ]),
    };
}

function stepOf(step: StepFile, folder: string): Step {
    const { thinking, text, calls = [], error, waitMs, stallMs, usage } = step;
    const holdUntil =
        step.holdUntil === undefined
            ? undefined
            : resolve(folder, step.holdUntil);
    const fill =
        step.fill === undefined ? undefined : resolve(folder, step.fill);
    const answer: FauxResponseFactory = async (_context, options) => {
        const signal = options?.signal;
        if (stallMs !== undefined) await sleep(stallMs);
        if (waitMs !== undefined)
            await sleep(waitMs, undefined, { signal }).catch(() => {});
        if (holdUntil !== undefined) await held(holdUntil, signal);
        if (signal?.aborted)
            return fauxAssistantMessage("", { stopReason: "aborted" });
        const words = fill === undefined ? {} : await namesIn(fill);
        const content: FauxContentBlock[] = [
            ...(thinking === undefined ? [] : [fauxThinking(thinking)]),
            ...(text === undefined ? [] : [fauxText(text)]),
            ...calls.map((call) =>
                fauxToolCall(
                    call.name,
                    filled(call.arguments, words) as ToolCall["arguments"],
                ),
            ),
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
                } else {
                    used.input += used.cacheWrite;
                    used.totalTokens += used.cacheWrite;
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
    const script: Script =
        path === undefined ? { steps: [], lists: new Map() } : scriptOf(path);
    const core = createFauxCore({
        models: [...(script.models ?? [{ id: FAUX_MODEL }])],
        ...(script.tokensPerSecond === undefined
            ? {}
            : { tokensPerSecond: script.tokensPerSecond }),
    });
    const stream: StreamFunction<string, SimpleStreamOptions> = (
        model,
        context,
        options,
    ) => {
        const step = stepsOf(script).shift() ?? { answer: echo };
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
