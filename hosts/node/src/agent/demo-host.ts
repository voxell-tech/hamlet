/**
 * The demo's Node host: the agent actor over the link, its model the faux
 * one of `faux.ts`, answering from the script `--faux <script>` names and
 * then from that file's lines. Given `--sign-in`, it also lists two
 * providers to sign in to, on loopback. It writes its pid to `demo-node.pid`
 * in the realm folder, and gives the program the demo's own actions below.
 */
// Only Node's own modules and the native module are imported before the
// take, as in `src/main.ts`.
import type { ActionCaller, Calls } from "../actor/actor/mod.ts";
import { fail, serve, take } from "../entry.ts";
import type { AgentContext } from "./actor.ts";
import type { AgentFilesystem } from "./filesystem.ts";

const ENTRY = "hamlet demo agent host";

take(ENTRY);

const home = process.env.HAMLET_HOME;
if (home === undefined) fail(ENTRY, "HAMLET_HOME names no realm folder");

const [{ actor, Address }, { wrongCaller }, { dying }, { hostedAgent }] =
    await Promise.all([
        import("../actor/actor/mod.ts"),
        import("./actor.ts"),
        import("./faux.ts"),
        import("./host.ts"),
    ]);

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
const { config } = hostedAgent(home, {
    ...(args.includes("--faux")
        ? { faux: args[args.indexOf("--faux") + 1] }
        : {}),
    signIn: args.includes("--sign-in"),
    claude: true,
    files: dyingIn,
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
