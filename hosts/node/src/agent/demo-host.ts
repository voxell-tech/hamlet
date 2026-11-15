// Only Node's own modules and the native module are imported before the
// take, as in `src/main.ts`.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import type {
    AssistantMessage,
    FauxProviderHandle,
    FauxResponseFactory,
    Message,
    Models,
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

const [{ serveLink }, { agentActor }, { openAgentFilesystem }, ai] =
    await Promise.all([
        import("../actor/registry/link.ts"),
        import("./actor.ts"),
        import("./filesystem.ts"),
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

const answer: FauxResponseFactory = (context): AssistantMessage => {
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

/**
 * The faux provider takes a step off its queue when a request starts and
 * runs it later, so a step that queued the next one itself would leave none
 * after a request that never ran it.
 */
function answering(models: Models, faux: FauxProviderHandle): Models {
    const queued = new Set<PropertyKey>([
        "stream",
        "streamSimple",
        "complete",
        "completeSimple",
    ]);
    return new Proxy(models, {
        get(target, key, receiver) {
            const value: unknown = Reflect.get(target, key, receiver);
            if (typeof value !== "function") return value;
            if (!queued.has(key)) return value.bind(target);
            return (...args: unknown[]) => {
                if (faux.getPendingResponseCount() === 0) {
                    faux.appendResponses([answer]);
                }
                return value.apply(target, args);
            };
        },
    });
}

function dyingIn(files: AgentFilesystem): AgentFilesystem {
    return {
        ...files,
        edit: (path, edits, context) => {
            if (!dying.delete(path)) return files.edit(path, edits, context);
            process.kill(process.pid, "SIGKILL");
            // The signal may land after `kill` returns: nothing is written before it.
            return new Promise(() => {});
        },
    };
}

const faux = ai.fauxProvider();
const models = ai.createModels();
models.setProvider(faux.provider);
const { provider, id } = faux.getModel();

const agent = agentActor({
    models: answering(models, faux),
    model: `${provider}/${id}`,
    files: (n) => dyingIn(openAgentFilesystem(home, n)),
});

try {
    await serveLink({ agent });
} catch (error) {
    fail(reason(error));
}
process.exit(0);
