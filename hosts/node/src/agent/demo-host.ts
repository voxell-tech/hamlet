/**
 * The demo's Node host: the agent actor over the link, its model a faux one.
 * The model answers the demo's own lines (`run:`, `tell`, `read` and the
 * others below). Given `--faux <script>`, it first answers with the script's
 * steps, one per request in order, and then with those lines.
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
import { takeLink } from "@hamlet/actor-node";
import type { AgentFilesystem } from "./filesystem.ts";

const NO_LINK =
    "this process has no link to a Rust host. The demo's Rust host starts it itself, with nothing between them: set HAMLET_NODE to the Node executable itself.";

/** A kill test reads it to tell this process from the next one. */
const PID_FILE = "demo-node.pid";

function fail(words: string): never {
    console.error(`hamlet demo agent host: ${words}`);
    process.exit(1);
}

function reason(error: unknown): string {
    if (!(error instanceof Error)) return String(error);
    const metadata: unknown = "metadata" in error ? error.metadata : undefined;
    const cause =
        typeof metadata === "object" && metadata !== null && "error" in metadata
            ? `: ${String(metadata.error)}`
            : "";
    return `${error.message}${cause}`;
}

try {
    takeLink();
} catch {
    fail(NO_LINK);
}

const home = process.env.HAMLET_HOME;
if (home === undefined) fail("HAMLET_HOME names no realm folder");
writeFileSync(join(home, PID_FILE), `${process.pid}\n`);

const [
    { serveLink },
    { agentActor },
    { openAgentFilesystem },
    { signIns },
    ai,
] = await Promise.all([
    import("../actor/registry/link.ts"),
    import("./actor.ts"),
    import("./native-filesystem.ts"),
    import("./login.ts"),
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

const echo: FauxResponseFactory = (context): AssistantMessage => {
    const last = context.messages.at(-1);
    if (last === undefined) return ai.fauxAssistantMessage("Heard nothing.");
    const words = wordsOf(last);
    if (last.role !== "user") return ai.fauxAssistantMessage(`Done: ${words}`);
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

/** Each answer with `usage` in place of the faux model's estimate, priced. */
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

function dyingIn(files: AgentFilesystem): AgentFilesystem {
    return {
        ...files,
        edit: async (path, edits, context) => {
            const edited = await files.edit(path, edits, context);
            if (!dying.delete(path)) return edited;
            process.kill(process.pid, "SIGKILL");
            // The signal may land after `kill` returns: nothing is answered before it.
            return new Promise(() => {});
        },
    };
}

const [flag, path] = process.argv.slice(2);
const script: Script =
    flag === "--faux" && path !== undefined ? scriptOf(path) : { steps: [] };
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
const models = ai.createModels();
models.setProvider(
    ai.createProvider({
        id: core.provider,
        auth: { apiKey: { name: "Faux", resolve: async () => ({ auth: {} }) } },
        models: core.models,
        api: { stream, streamSimple: stream },
    }),
);
const { provider, id } = core.getModel();

const agent = agentActor({
    models,
    model: `${provider}/${id}`,
    listed: () => models.getModels(),
    files: (n) => dyingIn(openAgentFilesystem(home, n)),
    signIns: signIns(models),
});

try {
    await serveLink({ agent });
} catch (error) {
    fail(reason(error));
}
process.exit(0);
