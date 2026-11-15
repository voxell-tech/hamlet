/**
 * The agent actor, `agent/<n>`: the ported wrapper on pi-durable, composed
 * from the parts `src/pi/runtime.ts` exports, with a surface of its own
 * messages only. `pi()` is not used: it adds its built-in actions, which
 * check no sender, and it reserves `submit`.
 *
 * At each wake the actor builds one `PiOptions` for that wake and keeps it
 * in its vars: a registry of the parts, each built from the agent's peers,
 * and the gate around the model access. Every call into the wrapper passes
 * that one object, so a harness the wrapper reopens after a failed commit
 * opens with the same parts.
 *
 * The actions reach the harness through the wrapper's `withHarness`, which
 * recovers a harness after a failed commit. The harness's own work, the
 * tools, hooks and the gate, reads it live instead (`liveHarness`).
 */
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { Models } from "@earendil-works/pi-ai";
import {
    createRegistry,
    type Extension,
    type Harness,
    type HarnessSettings,
    type Registry,
} from "@earendil-works/pi-durable";
import {
    type ActionCaller,
    type ActorDefinition,
    ActorError,
    Address,
    actor,
    type Caller,
} from "../actor/actor/mod.ts";
import { db, type RawAccess } from "../actor/db/mod.ts";
import { sharedActorConfig } from "../pi/actor-config.ts";
import {
    closeHarness,
    createPiRuntime,
    openOnWake,
    PI_RUNTIME,
    type PiContext,
    type PiDatabaseProvider,
    type PiOptions,
    piRuntime,
    resumeDueSleeps,
    withHarness,
} from "../pi/runtime.ts";
import { migratePiTables } from "../pi/storage.ts";
import type { AgentFilesystem } from "./filesystem.ts";
import {
    type AgentActions,
    type AgentInput,
    type Fill,
    type Finished,
    type Place,
    type RealmTask,
    read,
    SENDERS,
    type SocietyTool,
    type Submit,
} from "./messages.ts";
import { ownFiles } from "./own-files.ts";
import { society } from "./society.ts";
import { gated } from "./spend.ts";
import { timers } from "./timers.ts";
import {
    type Admission,
    Closing,
    conversationOf,
    draftOf,
    type Peers,
    peersOf,
    reasonOf,
    taskOf,
    turnsOf,
    type WithHarness,
} from "./turn.ts";
import { endWords, handTools, workspaceTools } from "./workspace-tools.ts";

/** What the Node host gives the agent kind when it registers it. */
export interface AgentConfig {
    /**
     * The model access of every agent of the kind. The gate wraps it at each
     * wake. P6's seam: `models.ts` builds it, with the model a chief chose.
     */
    readonly models: Models;
    /** The model of a new conversation, as `provider/modelId`. P6's seam. */
    readonly model: string;
    /** Opens the filesystem of agent `agent`. */
    readonly files: (agent: number) => AgentFilesystem;
    /**
     * Fills an agent's filesystem from a profile's branch. A seam: no
     * message on any branch lets an agent read a profile yet, so by default
     * a fill writes nothing.
     */
    readonly fill?: (files: AgentFilesystem, fill: Fill) => Promise<void>;
    /**
     * pi-durable's run settings. P6's seam, for compaction. `followUpMode`
     * is pinned over it (see `wake`).
     */
    readonly settings?: HarnessSettings;
}

/** The context of the agent's wake, which lives as long as the harness it opens. */
export type AgentContext = PiContext;

/** The context of one of the agent's actions. */
export type AgentCall = PiContext & ActionCaller;

declare const taken: unique symbol;

/**
 * An action as the actor registers it: it checked its sender, and it
 * answers `null`. Only `sentBy` makes one, so no action can answer anything
 * else, nor skip the check.
 */
export type Taken<Args extends readonly unknown[]> = ((
    c: AgentCall,
    ...args: Args
) => Promise<null>) & { readonly [taken]: true };

/** The agent's whole surface: its messages' actions, and the host's alarm. */
export type AgentHandlers = {
    readonly [N in keyof AgentActions]: Taken<Parameters<AgentActions[N]>>;
} & { readonly pi: { readonly wake: Taken<[]> } };

export type AgentDefinition = ActorDefinition<
    undefined,
    undefined,
    undefined,
    Record<symbol, unknown>,
    AgentInput,
    PiDatabaseProvider,
    Record<never, never>,
    Record<never, never>,
    AgentHandlers
>;

/**
 * The agent's parts. A part is one line here, and its order among the
 * others decides only the order its tools are offered in.
 *
 * P6's seam: the prompt's sections are one more extension in this list.
 */
function parts(peers: Peers, off: readonly SocietyTool[]): Extension[] {
    return [
        workspaceTools(peers),
        ownFiles(peers),
        society(peers, off),
        timers(),
    ];
}

/**
 * The agent's actions. Each names who sends it, in `SENDERS` or here, and
 * reads its message with its reader in `read`. Nothing else is on the
 * surface.
 */
function agentActions(config: AgentConfig): AgentHandlers {
    return {
        ...taking<AgentActions>(SENDERS, read, {
            submit,
            finished,
            place,
            fill: (c, message) => fill(c, config, message),
        }),
        pi: { wake: sentBy(ALARM, "pi.wake", () => [], wakeTimers) },
    };
}

/** The actor kind `agent`. */
export function agentActor(config: AgentConfig): AgentDefinition {
    // The config is built from the wrapper's untyped parts, as `pi()`
    // builds its own, so `actor()` cannot check it. `AgentHandlers` is what
    // holds the actions, and the declared return type is the contract.
    const definition: unknown = actor({
        ...sharedActorConfig(
            "agent",
            {},
            { slot: PI_RUNTIME, create: createPiRuntime },
            closeHarness,
            { onSleep: undefined, onDestroy: undefined },
        ),
        db: db({ onMigrate: migrateAgent }),
        onCreate: (c: AgentContext, input: unknown) =>
            create(c, config, read.agentInput(input)),
        onWake: (c: AgentContext) => wake(c, config),
        actions: agentActions(config),
    });
    return definition as AgentDefinition;
}

// ---------------------------------------------------------------------------
// The sender rule and the answer, written once
// ---------------------------------------------------------------------------

/** The sender of `pi.wake`: the host's alarm, which calls with no caller. */
const ALARM = null;

/**
 * `handle` as an action that only `sender` may call. A wrong caller gets
 * `message.wrong_caller`, and neither `read` nor `handle` runs. Then `read`
 * reads what was sent, and `handle` takes it. It answers `null` explicitly:
 * an action that returns nothing answers `["$Undefined", 0]`, which a Rust
 * `()` cannot read.
 */
function sentBy<Args extends readonly unknown[]>(
    sender: Address | typeof ALARM,
    action: string,
    read: (raw: readonly unknown[]) => Args,
    handle: (c: AgentCall, ...args: Args) => void | Promise<void>,
): Taken<Args> {
    const takenBy = async (c: AgentCall, ...raw: unknown[]): Promise<null> => {
        const caller = c.caller;
        const right =
            sender === ALARM
                ? caller === null
                : caller !== null &&
                  caller !== "client" &&
                  Address.equals(caller.actor, sender);
        if (!right) throw wrongCaller(action, caller);
        await handle(c, ...read(raw));
        return null;
    };
    return takenBy as unknown as Taken<Args>;
}

/**
 * Each handler of `handlers` as an action that only its sender in `senders`
 * may call, and whose message its reader in `readers` reads first.
 */
function taking<A extends { [N in keyof A]: (message: never) => unknown }>(
    senders: { readonly [N in keyof A]: Address },
    readers: { readonly [N in keyof A]: (raw: unknown) => Parameters<A[N]>[0] },
    handlers: {
        readonly [N in keyof A]: (
            c: AgentCall,
            message: Parameters<A[N]>[0],
        ) => void | Promise<void>;
    },
): { readonly [N in keyof A]: Taken<Parameters<A[N]>> } {
    const actions = {} as { [N in keyof A]: Taken<Parameters<A[N]>> };
    for (const name of Object.keys(handlers) as (keyof A & string)[]) {
        const reader = readers[name];
        actions[name] = sentBy(
            senders[name],
            name,
            (raw): [Parameters<A[typeof name]>[0]] => [reader(raw[0])],
            handlers[name],
        ) as unknown as Taken<Parameters<A[typeof name]>>;
    }
    return actions;
}

/** The error of `crates/hamlet_messages/src/sender.rs`, with its group, code and message. */
function wrongCaller(action: string, caller: Caller | null): ActorError {
    return new ActorError(
        "message",
        "wrong_caller",
        "The caller may not send this message.",
        {
            public: true,
            metadata: { action, ...(caller === null ? {} : { caller }) },
        },
    );
}

// ---------------------------------------------------------------------------
// The agent's own row
// ---------------------------------------------------------------------------

/**
 * The agent's file: the wrapper's tables, and the one table of the agent's
 * own, `agent`, with one row. It holds what the create input gave, and
 * which fill it took. A table and not `c.state`, so a place the realm was
 * answered for is never lost to a throttled state save.
 */
async function migrateAgent(raw: Pick<RawAccess, "execute">): Promise<void> {
    await migratePiTables(raw);
    await raw.execute(`
        CREATE TABLE IF NOT EXISTS agent (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            kind TEXT NOT NULL,
            task TEXT NOT NULL,
            place_version INTEGER NOT NULL,
            tools_off TEXT NOT NULL CHECK (json_valid(tools_off)),
            place TEXT NOT NULL CHECK (json_valid(place)),
            filled TEXT
        ) STRICT
    `);
}

/** The part of the row that a wake reads. */
interface Row {
    /** The standing task. */
    readonly task: RealmTask;
    readonly toolsOff: readonly SocietyTool[];
}

async function rowAt(c: AgentContext): Promise<Row> {
    const row = await c.db.get<{ task: string; tools_off: string }>(
        "SELECT task, tools_off FROM agent WHERE id = 1",
    );
    if (row === undefined)
        throw new Error("the agent has no row: it was not created");
    return {
        task: row.task,
        toolsOff: JSON.parse(row.tools_off) as SocietyTool[],
    };
}

/** The agent's number: its key, in decimal with no sign and no leading zero. */
function numberOf(c: AgentContext): number {
    const key = c.address.key;
    if (!/^[1-9][0-9]*$/.test(key))
        throw new Error(`\`${key}\` is not an agent's number`);
    return Number(key);
}

/** A place as the row keeps it: its version beside it, since a `u64` may be a bigint. */
function rowOf(
    place: Place,
): [version: number | bigint, toolsOff: string, place: string] {
    const { version, ...rest } = place;
    return [version, JSON.stringify(place.tools_off), JSON.stringify(rest)];
}

/** `onCreate`: writes the row from the create input, and makes the folders of the agent's filesystem. */
async function create(
    c: AgentContext,
    config: AgentConfig,
    input: AgentInput,
): Promise<void> {
    await c.db.run(
        "INSERT OR IGNORE INTO agent (id, kind, task, place_version, tools_off, place) VALUES (1, ?, ?, ?, ?, ?)",
        input.kind,
        input.task,
        ...rowOf(input.place),
    );
    config.files(numberOf(c));
}

// ---------------------------------------------------------------------------
// One wake
// ---------------------------------------------------------------------------

/** What a wake builds and every action of that wake uses. Kept in the actor's vars. */
interface Generation {
    /** The one `PiOptions` of this wake. Every call into the wrapper passes it. */
    readonly options: PiOptions;
    readonly peers: Peers;
    /** The registry in `options`. `place` installs the society extension into it again. */
    readonly registry: Registry;
}

/** The slot of the wake's `Generation` in the actor's vars, beside the wrapper's own. */
const AGENT: unique symbol = Symbol.for("@hamlet/agent");

function generationOf(c: PiContext): Generation {
    const generation = (c.vars as Record<symbol, Generation | undefined>)[
        AGENT
    ];
    if (generation === undefined) throw new Error("the agent has not woken");
    return generation;
}

/**
 * `onWake`: builds this wake's parts from the agent's peers, opens the
 * harness with them, and resumes what a kill or a sleep cut off.
 */
async function wake(c: AgentContext, config: AgentConfig): Promise<void> {
    const row = await rowAt(c);
    const registry = createRegistry();
    const peers = peersOf({
        calls: c,
        files: config.files(numberOf(c)),
        turns: turnsOf(liveHarness(c), row.task),
        report: (what, error) =>
            c.log.warn({ msg: what, error: reasonOf(error) }),
    });
    for (const part of parts(peers, row.toolsOff)) registry.install(part);
    const options: PiOptions = {
        registry,
        // Every provider request of this agent asks the realm and is charged.
        models: gated(config.models, peers),
        model: config.model,
        // No environment: files go through the tools, never through the
        // wrapper's database env.
        env: () => undefined,
        settings: {
            ...config.settings,
            // pi-durable's own default, pinned: one follow-up starts each
            // run, so a run's first input is its only input, and the turn's
            // task and the reply read `run.inputs[0]` alone. P6's seam:
            // steering puts a second input in a run, and must revisit
            // `Turns` and the reply hook when it lands.
            followUpMode: "one-at-a-time",
        },
    };
    (c.vars as Record<symbol, Generation>)[AGENT] = {
        options,
        peers,
        registry,
    };
    let harness: Harness;
    try {
        harness = await openOnWake(c, options);
    } catch (error) {
        // As `pi()` does: the actor stays up, and the next action opens
        // through `withHarness` and reports the cause.
        c.log.error({
            msg: "the agent's harness could not open on wake; actions will retry",
            error,
        });
        return;
    }
    // Not awaited, so a slow workspace does not hold `submit`'s 4 s.
    void handTools(peers).then(
        (extension) => registry.install(extension),
        (error: unknown) =>
            peers.report("the hand tools were not listed", error),
    );
    harness.resume();
}

/**
 * The harness open now, read live as the wrapper keeps it, for the harness's
 * own work: the tools, the hooks and the gate. It never recovers a harness,
 * as the wrapper's `withHarness` does: a recovery closes the harness, and a
 * close waits for the very work that would be waiting on it.
 */
function liveHarness(c: AgentContext): WithHarness {
    return async (use) => {
        const runtime = piRuntime(c);
        if (runtime.status !== "open") {
            throw new Closing(
                `the agent's harness is closing (${runtime.status})`,
            );
        }
        const open = await runtime.harness;
        if (open === undefined)
            throw new Error("the agent's harness is not open");
        return use(open.harness, BACKGROUND_CONTEXT);
    };
}

// ---------------------------------------------------------------------------
// The actions
// ---------------------------------------------------------------------------

/**
 * `submit{id, task?, input}`, from the realm. Admits the input once under
 * `id`, as a follow-up when the agent is busy, and answers when the
 * admission is committed, not when the turn ends.
 *
 * An `answer` goes to the conversation it names when that exists, and every
 * other input to the root. P6's seam: sessions choose the conversation here.
 */
async function submit(c: AgentCall, message: Submit): Promise<void> {
    await withHarness(c, generationOf(c).options, async (harness, context) => {
        const named =
            message.input.type === "answer"
                ? conversationOf(message.input.conversation)
                : undefined;
        const conversation =
            (named === undefined
                ? undefined
                : await harness.conversation(named, context)) ??
            (await harness.root(context));
        await conversation.submit(draftOf(admissionOf(message)), context);
    });
}

/**
 * `finished{call, outcome}`, from the workspace. Admits the end once under
 * the request id `call`, to the conversation of the tool call that made it,
 * found through its task `t<id>`. A call of no known task goes to the root.
 * Its content is the end as the tool would have worded it.
 */
async function finished(c: AgentCall, message: Finished): Promise<void> {
    await withHarness(c, generationOf(c).options, async (harness, context) => {
        const task = taskOf(message.call);
        const record =
            task === undefined
                ? undefined
                : await harness.getTask(task, context);
        const conversation =
            (record === undefined
                ? undefined
                : await harness.conversation(record.conversationId, context)) ??
            (await harness.root(context));
        await conversation.submit(
            draftOf({
                kind: "finished",
                id: message.call,
                content: endWords(message.outcome),
            }),
            context,
        );
    });
}

/**
 * `place{version, ...}`, from the realm. Keeps the place when its version
 * is newer, and then installs the society extension again without the tools
 * it turns off, so they leave the next request. An older or equal version
 * changes nothing.
 */
async function place(c: AgentCall, message: Place): Promise<void> {
    const [version, toolsOff, rest] = rowOf(message);
    const kept = await c.db.run(
        "UPDATE agent SET place_version = ?, tools_off = ?, place = ? WHERE id = 1 AND place_version < ?",
        version,
        toolsOff,
        rest,
        version,
    );
    if (kept.changes === 0) return;
    const { registry, peers } = generationOf(c);
    registry.install(society(peers, message.tools_off));
}

/**
 * `fill{profile, branch}`, from the realm. Fills the agent's filesystem
 * through `config.fill`, then keeps which fill it took. The same fill again
 * changes nothing and answers the same.
 */
async function fill(
    c: AgentCall,
    config: AgentConfig,
    message: Fill,
): Promise<void> {
    const wanted = JSON.stringify([message.profile, message.branch]);
    const row = await c.db.get<{ filled: string | null }>(
        "SELECT filled FROM agent WHERE id = 1",
    );
    if (row?.filled === wanted) return;
    await config.fill?.(generationOf(c).peers.files, message);
    await c.db.run("UPDATE agent SET filled = ? WHERE id = 1", wanted);
}

/** `pi.wake`, from the host's alarm: resumes pi-durable's due sleeps. The wrapper arms the next alarm. */
async function wakeTimers(c: AgentCall): Promise<void> {
    await withHarness(c, generationOf(c).options, () => resumeDueSleeps(c));
}

/**
 * The admission of a `submit`: its words for the model, and what its entry
 * keeps. P6's seam: a line's images and files are ids here, and the prompt
 * decides how a sender is named.
 */
function admissionOf(message: Submit): Admission {
    const { input } = message;
    const base = {
        id: message.id,
        ...(message.task === undefined ? {} : { task: message.task }),
    };
    const from = (author: { agent: number; name?: string }) =>
        author.name === undefined
            ? `\`hire#${author.agent}\``
            : `${author.name} (\`hire#${author.agent}\`)`;
    switch (input.type) {
        case "line":
            return { ...base, kind: "line", content: input.text };
        case "letter":
            return {
                ...base,
                kind: "letter",
                content: `A letter from ${from(input.from)}:\n\n${input.text}`,
            };
        case "question":
            return {
                ...base,
                kind: "question",
                question: input.letter,
                content: `A question from ${from(input.from)}. Your answer to this turn is sent back as the reply:\n\n${input.text}`,
            };
        case "answer":
            return {
                ...base,
                kind: "answer",
                content: `${from(input.from)} answers your question:\n\n${input.text}`,
            };
    }
}
