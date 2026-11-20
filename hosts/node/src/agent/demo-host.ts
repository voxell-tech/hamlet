/**
 * The demo's Node host: the agent actor over the link, its model a faux one.
 * The model answers the demo's own lines (`run:`, `tell`, `read` and the
 * others below), and the line `tools` with the tools it is offered, one
 * `<name>: <description>` a line. Given `--faux <script>`, it first answers
 * with the script's steps, one per request in order, and then with those
 * lines.
 *
 * A script is JSON, after today's server's `--faux` file:
 *
 * ```json
 * {
 *   "models": [{ "id": "faux-a", "input": 1, "output": 2, "window": 32000 }],
 *   "responses": [
 *     "Plain words.",
 *     { "text": "...", "calls": [{ "name": "bash", "arguments": {} }] },
 *     { "text": "Slow.", "waitMs": 20000 },
 *     { "error": "The model failed." },
 *     { "text": "Big.", "usage": { "input": 100000, "output": 10 } }
 *   ]
 * }
 * ```
 *
 * A model's `input` and `output` are its dollars per million tokens, and
 * `window` its context window; the agent runs the first one. A step's
 * `waitMs` is the time its answer takes, which stopping the request ends,
 * and its `usage` the tokens it says it used, in place of the faux model's
 * estimate. Each answer is priced from its model's rates, as a real
 * provider's is. A restarted host begins the script again.
 *
 * Given `--sign-in`, it also lists two providers to sign in to, on
 * loopback.
 */
// Only Node's own modules and the native module are imported before the
// take, as in `src/main.ts`.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import type {
    Api,
    AssistantMessage,
    AssistantMessageEventStream,
    FauxModelDefinition,
    FauxResponseFactory,
    Message,
    Model,
    SimpleStreamOptions,
    StreamFunction,
    ToolCall,
} from "@earendil-works/pi-ai";
import type { ActionCaller, Calls } from "../actor/actor/mod.ts";
import { fail, serve, take } from "../entry.ts";
import type { AgentContext } from "./actor.ts";
import type { AgentFilesystem } from "./filesystem.ts";

const ENTRY = "hamlet demo agent host";

/** A kill test reads it to tell this process from the next one. */
const PID_FILE = "demo-node.pid";

take(ENTRY);

const home = process.env.HAMLET_HOME;
if (home === undefined) fail(ENTRY, "HAMLET_HOME names no realm folder");
writeFileSync(join(home, PID_FILE), `${process.pid}\n`);

const [
    { actor, Address },
    { agentActor, wrongCaller },
    { openAgentFilesystem },
    { signIns },
    { claudeExecutable, withClaude },
    ai,
] = await Promise.all([
    import("../actor/actor/mod.ts"),
    import("./actor.ts"),
    import("./native-filesystem.ts"),
    import("./login.ts"),
    import("./claude.ts"),
    import("@earendil-works/pi-ai"),
]);

const dying = new Set<string>();

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
    if (last === undefined) return ai.fauxAssistantMessage("Heard nothing.");
    const words = wordsOf(last);
    if (last.role !== "user") return ai.fauxAssistantMessage(`Done: ${words}`);
    if (words === "tools") {
        return ai.fauxAssistantMessage(
            ai
                .getCurrentTools(context.messages)
                .map((tool) => `${tool.name}: ${tool.description}`)
                .join("\n"),
        );
    }
    for (const line of LINES) {
        const found = line.form.exec(words);
        if (found === null) continue;
        return ai.fauxAssistantMessage(
            ai.fauxToolCall(line.tool, line.args(...found.slice(1))),
            { stopReason: "toolUse" },
        );
    }
    return ai.fauxAssistantMessage(`Heard: ${words}`);
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

/** A script as its file holds it; the demo's lanes write it, so it is trusted. */
interface ScriptFile {
    readonly models?: readonly {
        readonly id: string;
        readonly input?: number;
        readonly output?: number;
        readonly window?: number;
    }[];
    readonly responses?: readonly (string | StepFile)[];
}

interface StepFile {
    readonly text?: string;
    readonly calls?: readonly {
        readonly name: string;
        readonly arguments: ToolCall["arguments"];
    }[];
    readonly error?: string;
    readonly waitMs?: number;
    readonly usage?: Tokens;
}

function scriptOf(path: string): Script {
    const file = JSON.parse(readFileSync(path, "utf8")) as ScriptFile;
    return {
        ...(file.models === undefined
            ? {}
            : {
                  models: file.models.map((model) => ({
                      id: model.id,
                      cost: {
                          input: model.input ?? 0,
                          output: model.output ?? 0,
                          cacheRead: 0,
                          cacheWrite: 0,
                      },
                      ...(model.window === undefined
                          ? {}
                          : { contextWindow: model.window }),
                  })),
              }),
        steps: (file.responses ?? []).map((step) =>
            typeof step === "string"
                ? { answer: () => ai.fauxAssistantMessage(step) }
                : stepOf(step),
        ),
    };
}

function stepOf(step: StepFile): Step {
    const { text, calls = [], error, waitMs, usage } = step;
    const answer: FauxResponseFactory = async (_context, options) => {
        const signal = options?.signal;
        if (waitMs !== undefined)
            await sleep(waitMs, undefined, { signal }).catch(() => {});
        if (signal?.aborted)
            return ai.fauxAssistantMessage("", { stopReason: "aborted" });
        const content = [
            ...(text === undefined ? [] : [ai.fauxText(text)]),
            ...calls.map((call) => ai.fauxToolCall(call.name, call.arguments)),
        ];
        if (error !== undefined)
            return ai.fauxAssistantMessage(content, {
                stopReason: "error",
                errorMessage: error,
            });
        return ai.fauxAssistantMessage(content, {
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
    const out = ai.createAssistantMessageEventStream();
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
                used.cost = ai.calculateCost(model, used);
            }
            out.push(event);
        }
        out.end(await source.result());
    })();
    return out;
}

/** Kills this process; nothing is answered before the signal lands. */
function die(): Promise<never> {
    process.kill(process.pid, "SIGKILL");
    // The signal may land after `kill` returns.
    return new Promise(() => {});
}

function dyingIn(files: AgentFilesystem): AgentFilesystem {
    return {
        ...files,
        edit: async (path, edits, context) => {
            const edited = await files.edit(path, edits, context);
            if (!dying.delete(path)) return edited;
            return die();
        },
    };
}

/** The realm's actions whose next answer kills this process once it arrives. */
const dyingAfter = new Set<string>();

/**
 * `c`, whose calls of the realm kill this process once the realm answered an
 * action of `dyingAfter`, before the caller reads the answer.
 */
function dyingAfterCalls(c: AgentContext): AgentContext {
    const call: Calls["call"] = async (to, name, args, options) => {
        const answer = await c.call(to, name, args, options);
        if (to.kind === "realm" && dyingAfter.delete(name)) return die();
        return answer;
    };
    return new Proxy(c, {
        get: (target, key) => {
            if (key === "call") return call;
            const value: unknown = Reflect.get(target, key, target);
            return typeof value === "function" ? value.bind(target) : value;
        },
    });
}

/** Shorter than the demo's own call of `forward`. */
const FORWARD_MS = 20_000;

type DemoCall = AgentContext & ActionCaller;

/** Throws as every other action does unless the program made the call. */
function fromProgram(c: DemoCall, action: string): void {
    if (c.caller !== null) throw wrongCaller(action, c.caller);
}

/** The program's own actions on an agent, for the demo's commands. */
const demoActions = {
    /** Calls `name` of the actor at `to` as this agent, and answers its answer. */
    forward: async (
        c: DemoCall,
        to: { readonly kind: string; readonly key: string },
        name: string,
        args: readonly unknown[],
    ): Promise<unknown> => {
        fromProgram(c, "forward");
        const answer = await c.call(Address.of(to.kind, to.key), name, args, {
            deadlineMs: FORWARD_MS,
        });
        return answer ?? null;
    },
    /** The agent's standing task and the inputs in its transcript, oldest first. */
    taken: async (c: DemoCall) => {
        fromProgram(c, "taken");
        const standing = await c.db.get<{ task: string }>(
            "SELECT task FROM agent WHERE id = 1",
        );
        const inputs = await c.db.all<{
            taken: number;
            kind: string;
            task: string | null;
            question: number | null;
            text: string;
        }>(
            "SELECT id AS taken, substr(record ->> '$.kind', 8) AS kind, record ->> '$.data.task' AS task, record ->> '$.data.question' AS question, coalesce(record ->> '$.model', '') AS text FROM pi_entries WHERE record ->> '$.kind' LIKE 'hamlet.%' ORDER BY id",
        );
        return { standing: standing?.task ?? null, inputs };
    },
    /** Every entry of the agent's conversations, as its file holds it. */
    entries: async (c: DemoCall) => {
        fromProgram(c, "entries");
        return c.db.all<{ conversation: number; id: number; record: string }>(
            "SELECT conversation_id AS conversation, id, record FROM pi_entries ORDER BY id",
        );
    },
    /** Arms `dyingAfter` for the realm's next answer to `action`. */
    die_after: async (c: DemoCall, action: string) => {
        fromProgram(c, "die_after");
        dyingAfter.add(action);
        return null;
    },
};

const args = process.argv.slice(2);
const path = args.includes("--faux")
    ? args[args.indexOf("--faux") + 1]
    : undefined;
const script: Script = path === undefined ? { steps: [] } : scriptOf(path);
const core = ai.createFauxCore(
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
    return measured(core.stream(model, context, options), model, step.usage);
};

const DEMO_OAUTH = {
    access: 'made-up-oauth-access-"demo',
    refresh: 'made-up-oauth-refresh-"demo',
};

function signInProviders() {
    const modelsOf = (provider: string) =>
        ai.createFauxCore({
            provider,
            api: core.api,
            models: [{ id: "large" }],
        }).models;
    const api = { stream, streamSimple: stream };
    const loopback = ai.createProvider({
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
    const chatgpt = ai.createProvider({
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
                        ...DEMO_OAUTH,
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

const models = ai.createModels();
models.setProvider(
    ai.createProvider({
        id: core.provider,
        auth: { apiKey: { name: "Faux", resolve: async () => ({ auth: {} }) } },
        models: core.models,
        api: { stream, streamSimple: stream },
    }),
);
if (args.includes("--sign-in")) {
    for (const provider of signInProviders()) models.setProvider(provider);
}
const executable = claudeExecutable();
if (executable !== undefined)
    withClaude(models, { executable, folder: join(home, "claude") });
const { provider, id } = core.getModel();

const { config } = agentActor({
    models,
    model: `${provider}/${id}`,
    listed: () => models.getModels(),
    files: (n) => dyingIn(openAgentFilesystem(home, n)),
    library: join(home, "library"),
    signIns: signIns(models),
});

// Built from the agent's untyped parts, as `agentActor` builds it.
const parts: object = config;
await serve(ENTRY, {
    agent: actor({
        ...parts,
        onWake: (c: AgentContext) => config.onWake?.(dyingAfterCalls(c)),
        actions: { ...config.actions, ...demoActions },
    }),
});
