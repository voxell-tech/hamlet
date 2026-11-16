// Not `pi()`: its built-in actions check no sender, and it reserves `submit`.
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { Api, Model, Models } from "@earendil-works/pi-ai";
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
import { type ChatHandlers, chatActions } from "./chat.ts";
import { compactionFor, cutOff } from "./compaction.ts";
import {
    copiesOf,
    keep,
    migrateCopies,
    type ReadCopies,
    refreshed,
} from "./copies.ts";
import type { AgentFilesystem } from "./filesystem.ts";
import type { SignIns } from "./login.ts";
import {
    type AgentActions,
    type AgentInput,
    type Copies,
    type Fill,
    type Finished,
    type Place,
    type RealmTask,
    read,
    SENDERS,
    type Submit,
    type ToolName,
} from "./messages.ts";
import { agentModels, configured } from "./models.ts";
import { ownFiles } from "./own-files.ts";
import { type PromptSources, promptPart } from "./prompt.ts";
import { owedOf, reflectEvery, reflection } from "./reflect.ts";
import { migrateChats } from "./sessions.ts";
import { society } from "./society.ts";
import { gated } from "./spend.ts";
import { timers } from "./timers.ts";
import { type Following, following, watching } from "./transcript.ts";
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

export interface AgentConfig {
    readonly models: Models;
    /** As `provider/modelId`. */
    readonly model: string;
    readonly files: (agent: number) => AgentFilesystem;
    readonly fill?: (files: AgentFilesystem, fill: Fill) => Promise<void>;
    readonly settings?: HarnessSettings;
    /** The models the listing holds now, which compaction is sized by. */
    readonly listed?: () => readonly Model<Api>[];
    /** Answers the Hand's `login`; a host without it signs no one in. */
    readonly signIns?: SignIns;
}

export type AgentContext = PiContext;

export type AgentCall = PiContext & ActionCaller;

declare const taken: unique symbol;

/**
 * Only `sentBy` makes one, so every action checks its sender and answers
 * `null`.
 */
export type Taken<Args extends readonly unknown[]> = ((
    c: AgentCall,
    ...args: Args
) => Promise<null>) & { readonly [taken]: true };

export type AgentHandlers = {
    readonly [N in keyof AgentActions]: Taken<Parameters<AgentActions[N]>>;
} & ChatHandlers & { readonly pi: { readonly wake: Taken<[]> } };

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

/** Their order decides only the order the tools are offered in. */
function parts(
    peers: Peers,
    off: readonly ToolName[],
    prompt: PromptSources,
    withHarness: WithHarness,
): Extension[] {
    return [
        promptPart(prompt),
        reflection(prompt.copies, owedOf(withHarness), reflectEvery()),
        cutOff(),
        workspaceTools(peers),
        ownFiles(peers),
        society(peers, off),
        timers(),
    ];
}

function agentActions(config: AgentConfig): AgentHandlers {
    return {
        ...taking<AgentActions>(SENDERS, read, {
            submit,
            finished,
            place,
            fill: (c, message) => fill(c, config, message),
            copies,
        }),
        ...chatActions(config),
        pi: { wake: sentBy(ALARM, "pi.wake", () => [], wakeTimers) },
    };
}

export function agentActor(config: AgentConfig): AgentDefinition {
    // Built from the wrapper's untyped parts, so `actor()` cannot check
    // it: the declared return type is the contract.
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

/** The host's alarm calls with no caller. */
const ALARM = null;

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
        // Returning nothing answers `["$Undefined", 0]`, which a Rust `()`
        // cannot read.
        return null;
    };
    return takenBy as unknown as Taken<Args>;
}

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

/** As the error of `crates/hamlet_messages/src/sender.rs`. */
export function wrongCaller(action: string, caller: Caller | null): ActorError {
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

/**
 * A table and not `c.state`, so a place the realm was answered for is never
 * lost to a throttled state save.
 */
async function migrateAgent(raw: Pick<RawAccess, "execute">): Promise<void> {
    await migratePiTables(raw);
    await migrateCopies(raw);
    await migrateChats(raw);
    await raw.execute(`
        CREATE TABLE IF NOT EXISTS agent (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            kind TEXT NOT NULL,
            task TEXT NOT NULL,
            place_version INTEGER NOT NULL,
            tools_off TEXT NOT NULL CHECK (json_valid(tools_off)),
            place TEXT NOT NULL CHECK (json_valid(place)),
            filled TEXT,
            epoch INTEGER NOT NULL DEFAULT 0
        ) STRICT
    `);
}

interface Row {
    readonly standingTask: RealmTask;
    readonly toolsOff: readonly ToolName[];
}

async function rowAt(c: AgentContext): Promise<Row> {
    const row = await c.db.get<{ task: string; tools_off: string }>(
        "SELECT task, tools_off FROM agent WHERE id = 1",
    );
    if (row === undefined)
        throw new Error("the agent has no row: it was not created");
    return {
        standingTask: row.task,
        toolsOff: JSON.parse(row.tools_off) as ToolName[],
    };
}

function numberOf(c: AgentContext): number {
    const key = c.address.key;
    if (!/^[1-9][0-9]*$/.test(key))
        throw new Error(`\`${key}\` is not an agent's number`);
    return Number(key);
}

function rowOf(
    place: Place,
): [version: number | bigint, toolsOff: string, place: string] {
    const { version, ...rest } = place;
    return [version, JSON.stringify(place.tools_off), JSON.stringify(rest)];
}

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

export interface Generation {
    /**
     * Every call into the wrapper passes this one object, so a harness it
     * reopens opens with the same parts.
     */
    readonly options: PiOptions;
    readonly peers: Peers;
    readonly registry: Registry;
    readonly copies: ReadCopies;
    readonly following: Following;
}

const AGENT: unique symbol = Symbol.for("@hamlet/agent");

export function generationOf(c: PiContext): Generation {
    const generation = (c.vars as Record<symbol, Generation | undefined>)[
        AGENT
    ];
    if (generation === undefined) throw new Error("the agent has not woken");
    return generation;
}

async function wake(c: AgentContext, config: AgentConfig): Promise<void> {
    const row = await rowAt(c);
    const registry = createRegistry();
    const withHarness = liveHarness(c);
    const peers = peersOf({
        calls: c,
        files: config.files(numberOf(c)),
        turns: turnsOf(withHarness, row.standingTask),
        report: (what, error) =>
            c.log.warn({ msg: what, error: reasonOf(error) }),
    });
    const copies = copiesOf(c.db);
    const prompt: PromptSources = { copies, peers };
    for (const part of parts(peers, row.toolsOff, prompt, withHarness)) {
        registry.install(part);
    }
    const options: PiOptions = {
        registry,
        models: gated(agentModels(config.models, copies, peers), peers),
        model: config.model,
        // Files go through the tools, never through the wrapper's database env.
        env: () => undefined,
        settings: {
            ...config.settings,
            // One follow-up starts each run, so a run's first input is its
            // only input, and the turn's task and the reply read
            // `run.inputs[0]` alone.
            followUpMode: "one-at-a-time",
            compaction: {
                ...compactionFor(config.listed?.() ?? []),
                ...config.settings?.compaction,
            },
        },
    };
    const follow = await following(c);
    (c.vars as Record<symbol, Generation>)[AGENT] = {
        options,
        peers,
        registry,
        copies,
        following: follow,
    };
    watching(c, follow);
    // Not awaited, so a slow workspace does not hold `submit` past the
    // realm's deadline. Begun
    // before the open, since an action that reopens a harness the open
    // left closed builds it from the same registry.
    void handTools(peers).then(
        (extension) => registry.install(extension),
        (error: unknown) =>
            peers.report("the hand tools were not listed", error),
    );
    let harness: Harness;
    try {
        harness = await openOnWake(c, options);
    } catch (error) {
        c.log.error({
            msg: "the agent's harness could not open on wake; actions will retry",
            error,
        });
        return;
    }
    harness.resume();
    void refreshed(c, peers).then(
        (kept) => (kept.has("model") ? configured(c, copies) : undefined),
        (error: unknown) =>
            peers.report("the copies were not read from the realm", error),
    );
}

/**
 * Never recovers a harness, as the wrapper's `withHarness` does: a recovery
 * closes the harness, and a close waits for the very work that would be
 * waiting on it.
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

/** Answers when the admission is committed, not when the turn ends. */
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
    // Installing it again replaces it, so the tools turned off leave the next
    // request.
    registry.install(society(peers, message.tools_off));
}

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

async function copies(c: AgentCall, message: Copies): Promise<void> {
    const kept = await keep(c, message);
    if (kept.has("model")) await configured(c, generationOf(c).copies);
}

async function wakeTimers(c: AgentCall): Promise<void> {
    await withHarness(c, generationOf(c).options, () => resumeDueSleeps(c));
}

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
