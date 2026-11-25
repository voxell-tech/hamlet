// Not `pi()`: its built-in actions check no sender, and it reserves `submit`.
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { Api, Model, Models } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    createRegistry,
    defineExtension,
    type Extension,
    GenerationTask,
    type Harness,
    type HarnessSettings,
    hook,
    type Registry,
    type SubmissionDraft,
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
import { type Branch, branchOf, migrateBranch } from "./branch.ts";
import { briefTurn } from "./brief.ts";
import { type ChatHandlers, chatActions } from "./chat.ts";
import { stop } from "./commands.ts";
import { compactionFor, cutOffRule } from "./compaction.ts";
import {
    copiesOf,
    keep,
    migrateCopies,
    type ReadCopies,
    refreshed,
} from "./copies.ts";
import {
    type FailedTurns,
    failedTurns,
    type TellFailed,
    type TurnEnded,
    tellingFailed,
} from "./failed.ts";
import type { AgentFilesystem } from "./filesystem.ts";
import { admissionOf } from "./inputs.ts";
import type { SignIns } from "./login.ts";
import {
    type AgentActions,
    type AgentInput,
    type Brief,
    type Copies,
    type Fill,
    type Finished,
    type Moved,
    type Place,
    type Removal,
    type RemovePath,
    read,
    SENDERS,
    type ShelvesChanged,
    type Submit,
} from "./messages/agent.ts";
import { readShelves } from "./messages/library.ts";
import { OWN_PROFILE, profileAt } from "./messages/profile.ts";
import type { SocietyTool, ToolName } from "./messages/realm.ts";
import type { ProfileId, RealmTask } from "./messages/shared.ts";
import { agentModels, configured, secretsRefreshed } from "./models.ts";
import { type Folder, ownFiles } from "./own-files.ts";
import { received } from "./parcels.ts";
import {
    type PromptSources,
    placingOf,
    promptPart,
    promptSources,
} from "./prompt.ts";
import { reflectEvery, reflection } from "./reflect.ts";
import {
    conversationFor,
    current,
    migrateChats,
    noticeEntry,
} from "./sessions.ts";
import { offeredSociety, society } from "./society.ts";
import { gated } from "./spend.ts";
import { timers } from "./timers.ts";
import { type Following, following, watching } from "./transcript.ts";
import {
    type Admission,
    Closing,
    conversationsOf,
    draftOf,
    isBusy,
    libraryCalls,
    type Peers,
    peersOf,
    reasonOf,
    taskOf,
    toolLister,
    turnsOf,
    type WithHarness,
} from "./turn.ts";
import {
    endOf,
    endWords,
    handTools,
    workspaceTools,
} from "./workspace-tools.ts";

export interface AgentConfig {
    readonly models: Models;
    /** As `provider/modelId`. */
    readonly model: string;
    readonly files: (agent: number) => AgentFilesystem;
    /** The library's checkout, by its absolute path, which the prompt names. */
    readonly library?: string;
    readonly settings?: HarnessSettings;
    /** The models the listing holds now. */
    readonly listed?: () => readonly Model<Api>[];
    /** Answers the Hand's `login`; a host without it signs no one in. */
    readonly signIns?: SignIns;
}

export type AgentContext = PiContext;

export type AgentCall = PiContext & ActionCaller;

declare const taken: unique symbol;

/**
 * Only `sentBy` makes one, so every action checks its sender, and answers
 * `null` unless its message has an answer.
 */
export type Taken<Args extends readonly unknown[], Answer = null> = ((
    c: AgentCall,
    ...args: Args
) => Promise<Answer>) & { readonly [taken]: true };

export type AgentHandlers = {
    readonly [N in keyof AgentActions]: Taken<
        Parameters<AgentActions[N]>,
        ReturnType<AgentActions[N]>
    >;
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

interface Offer {
    /** Whether the built-in or hand tool `name` is offered. */
    readonly builtIn: (name: string) => boolean;
    /** The society tools offered, with the timers and reflection; absent offers none. */
    readonly society?: readonly SocietyTool[];
}

/**
 * What an agent of its kind is offered, as today decides it
 * (`crates/hamlet_server/src/calls/place.rs:176-242`): an errand the built-in
 * tools its brief names, or no tool (`undefined`) when it names none, a worker
 * the built-in tools its brief names, or all of them when it names none, and a
 * Hand or a hire every tool but the built-in and hand tools its lead turned
 * off.
 */
function offerOf(row: Row): Offer | undefined {
    switch (row.kind) {
        case "errand": {
            const given = new Set(row.brief?.tools?.map(({ name }) => name));
            if (given.size === 0) return undefined;
            return { builtIn: (name) => given.has(name) };
        }
        case "worker": {
            const given = new Set(row.brief?.tools?.map(({ name }) => name));
            return { builtIn: (name) => given.size === 0 || given.has(name) };
        }
        case "hand":
        case "hire": {
            const off = new Set(row.toolsOff.map(({ name }) => name));
            return {
                builtIn: (name) => !off.has(name),
                society: offeredSociety(row.kind, row.place),
            };
        }
    }
}

/**
 * The built-in and hand tools as listed, each installed with only the tools
 * `offer` offers. A place that changes `offer` installs them again.
 */
interface BuiltIns {
    readonly listed: Extension[];
    offer: Offer;
}

function offered(extension: Extension, offer: Offer): Extension {
    return {
        ...extension,
        tools: (extension.tools ?? []).filter((tool) =>
            offer.builtIn(tool.name),
        ),
    };
}

/** Their order decides only the order the tools are offered in. */
function parts(
    builtIns: BuiltIns | undefined,
    turnEnded: TurnEnded,
    peers: Peers,
    prompt: PromptSources,
    withHarness: WithHarness,
    tell: TellFailed,
    failed: FailedTurns,
    chief: () => Promise<ConversationId | undefined>,
): Extension[] {
    if (builtIns === undefined) return [promptPart(prompt)];
    const builtIn = builtIns.listed.map((extension) =>
        offered(extension, builtIns.offer),
    );
    const { society: offeredTools } = builtIns.offer;
    if (offeredTools === undefined) {
        return [
            promptPart(prompt),
            cutOffRule(tell),
            failed.extension,
            ...builtIn,
        ];
    }
    return [
        promptPart(prompt),
        reflection(peers, prompt.copies, withHarness, reflectEvery(), chief),
        cutOffRule(tell),
        failed.extension,
        ...builtIn,
        society(peers, folderOf(prompt), offeredTools),
        timers(),
        saving(turnEnded),
    ];
}

/** The project folder the agent's place names now. */
function folderOf(prompt: PromptSources): Folder {
    return async () => (await prompt.where()).place.folder;
}

/** A turn that answered saves the filesystem to the branch. */
function saving(turnEnded: TurnEnded): Extension {
    return defineExtension({
        name: "hamlet.branch",
        hooks: [
            hook(GenerationTask, {
                afterRun: async (_run, api) => {
                    await turnEnded(api.conversationId);
                    return undefined;
                },
            }),
        ],
    });
}

function agentActions(config: AgentConfig): AgentHandlers {
    return {
        ...taking<AgentActions>(SENDERS, read, {
            submit,
            finished,
            place,
            fill,
            copies,
            moved,
            remove_path,
            shelves_changed,
            stop,
        }),
        ...chatActions(config),
        pi: { wake: sentBy<[], null>(ALARM, "pi.wake", () => [], wakeTimers) },
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

type Sender = Address | typeof OWN_PROFILE | typeof ALARM;

async function isSender(
    c: AgentCall,
    sender: Sender,
    caller: Caller | null,
): Promise<boolean> {
    if (sender === ALARM) return caller === null;
    if (caller === null || caller === "client") return false;
    if (sender !== OWN_PROFILE) return Address.equals(caller.actor, sender);
    const profile = await profileOf(c);
    return (
        profile !== undefined &&
        Address.equals(caller.actor, profileAt(profile))
    );
}

function sentBy<Args extends readonly unknown[], Answer = null>(
    sender: Sender,
    action: string,
    read: (raw: readonly unknown[]) => Args,
    handle: (
        c: AgentCall,
        ...args: Args
    ) => Promise<Answer> | Promise<void> | void,
): Taken<Args, Answer> {
    const takenBy = async (
        c: AgentCall,
        ...raw: unknown[]
    ): Promise<Answer | null> => {
        const caller = c.caller;
        if (!(await isSender(c, sender, caller))) {
            throw wrongCaller(action, caller);
        }
        // Returning nothing answers `["$Undefined", 0]`, which a Rust `()`
        // cannot read.
        return (await handle(c, ...read(raw))) ?? null;
    };
    return takenBy as unknown as Taken<Args, Answer>;
}

function taking<A extends { [N in keyof A]: (message: never) => unknown }>(
    senders: { readonly [N in keyof A]: Address | typeof OWN_PROFILE },
    readers: { readonly [N in keyof A]: (raw: unknown) => Parameters<A[N]>[0] },
    handlers: {
        readonly [N in keyof A]: (
            c: AgentCall,
            message: Parameters<A[N]>[0],
        ) => Promise<ReturnType<A[N]>> | Promise<void> | void;
    },
): { readonly [N in keyof A]: Taken<Parameters<A[N]>, ReturnType<A[N]>> } {
    const actions = {} as {
        [N in keyof A]: Taken<Parameters<A[N]>, ReturnType<A[N]>>;
    };
    for (const name of Object.keys(handlers) as (keyof A & string)[]) {
        const reader = readers[name];
        actions[name] = sentBy(
            senders[name],
            name,
            (raw): [Parameters<A[typeof name]>[0]] => [reader(raw[0])],
            handlers[name],
        ) as unknown as Taken<
            Parameters<A[typeof name]>,
            ReturnType<A[typeof name]>
        >;
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
    await migrateBranch(raw);
    await raw.execute(`
        CREATE TABLE IF NOT EXISTS agent (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            kind TEXT NOT NULL,
            task TEXT NOT NULL,
            place_version INTEGER NOT NULL,
            tools_off TEXT NOT NULL CHECK (json_valid(tools_off)),
            place TEXT NOT NULL CHECK (json_valid(place)),
            profile INTEGER,
            filled TEXT,
            epoch INTEGER NOT NULL DEFAULT 0,
            brief TEXT CHECK (brief IS NULL OR json_valid(brief))
        ) STRICT
    `);
}

interface Row {
    readonly kind: AgentInput["kind"];
    readonly standingTask: RealmTask;
    readonly toolsOff: readonly ToolName[];
    readonly place: Omit<Place, "version">;
    readonly brief: Brief | undefined;
}

export async function rowAt(c: AgentContext): Promise<Row> {
    const row = await c.db.get<{
        kind: AgentInput["kind"];
        task: string;
        tools_off: string;
        place: string;
        brief: string | null;
    }>("SELECT kind, task, tools_off, place, brief FROM agent WHERE id = 1");
    if (row === undefined)
        throw new Error("the agent has no row: it was not created");
    return {
        kind: row.kind,
        standingTask: row.task,
        toolsOff: JSON.parse(row.tools_off) as ToolName[],
        place: JSON.parse(row.place) as Omit<Place, "version">,
        brief:
            row.brief === null ? undefined : (JSON.parse(row.brief) as Brief),
    };
}

export function numberOf(c: AgentContext): number {
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
    await c.db.transaction(async (db) => {
        await db.run(
            "INSERT OR IGNORE INTO agent (id, kind, task, place_version, tools_off, place, brief) VALUES (1, ?, ?, ?, ?, ?, ?)",
            input.kind,
            input.task,
            ...rowOf(input.place),
            input.brief === undefined ? null : JSON.stringify(input.brief),
        );
        await keep({ db }, input.copies);
    });
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
    readonly prompt: PromptSources;
    readonly branch: Branch;
    readonly following: Following;
    /**
     * Runs `work` once every earlier `work` has settled. What reads the
     * current session and then writes by it runs here.
     */
    readonly inTurn: <T>(work: () => Promise<T>) => Promise<T>;
    /**
     * Admits letters and questions with files one after another, so two
     * never take one name and a repeat finds the first admitted.
     */
    readonly receiving: <T>(work: () => Promise<T>) => Promise<T>;
    /**
     * Settles once the last retry sent, queued or under way, is admitted
     * or has failed. A stop waits for it, so it stops the retry's turn too.
     */
    retrying: Promise<void> | undefined;
    /** Absent for an errand, which is offered no tool. */
    readonly builtIns?: BuiltIns;
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
    const branch = branchOf(c, peers, (except) =>
        withHarness(async (harness, context) => {
            for (const id of await conversationsOf(harness, context)) {
                if (id !== except && (await isBusy(harness, id, context))) {
                    return true;
                }
            }
            return false;
        }),
    );
    const errand =
        row.kind === "errand" ? (row.brief?.instructions ?? "") : undefined;
    const placing = placingOf(c, peers);
    const tools = toolLister(peers);
    const prompt: PromptSources = {
        ...promptSources(c, copies, peers, placing, tools),
        ...(config.library === undefined ? {} : { library: config.library }),
        ...(errand === undefined ? {} : { errand }),
    };
    const chief = () => current(c.db);
    const tell = tellingFailed(peers, row.standingTask, chief);
    const turnEnded = (conversation: ConversationId) =>
        branch
            .turnEnded(conversation)
            .catch((error: unknown) =>
                peers.report("the filesystem was not saved", error),
            );
    const failed = failedTurns(peers, withHarness, tell, turnEnded);
    const offer = offerOf(row);
    const builtIns: BuiltIns | undefined =
        offer === undefined
            ? undefined
            : {
                  listed: [
                      workspaceTools(peers),
                      ownFiles(peers, folderOf(prompt)),
                  ],
                  offer,
              };
    for (const part of parts(
        builtIns,
        turnEnded,
        peers,
        prompt,
        withHarness,
        tell,
        failed,
        chief,
    )) {
        registry.install(part);
    }
    const options: PiOptions = {
        registry,
        models: gated(
            agentModels(config.models, copies, peers),
            peers,
            placing,
        ),
        model: config.model,
        // Files go through the tools, never through the wrapper's database env.
        env: () => undefined,
        onReport: (report) =>
            peers.report(`pi-durable reported ${report.kind}`, report.error),
        settings: {
            ...config.settings,
            // One follow-up starts each run; a chief's line may join it
            // as a steer.
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
        prompt,
        branch,
        following: follow,
        inTurn: oneAfterAnother(),
        receiving: oneAfterAnother(),
        retrying: undefined,
        ...(builtIns === undefined ? {} : { builtIns }),
    };
    watching(c, follow);
    // Not awaited, so a slow workspace does not hold `submit` past the
    // realm's deadline. Begun
    // before the open, since an action that reopens a harness the open
    // left closed builds it from the same registry.
    if (builtIns !== undefined) {
        void handTools(peers, tools).then(
            (extension) => {
                builtIns.listed.push(extension);
                registry.install(offered(extension, builtIns.offer));
            },
            (error: unknown) =>
                peers.report("the hand tools were not listed", error),
        );
    }
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
    failed.follow(harness);
    harness.resume();
    const configuring = () =>
        configured(c, copies).catch((error: unknown) =>
            peers.report("the model was not configured", error),
        );
    // Done at every wake, so a configure a kill cut off is done again, and
    // before any action, so a first turn runs on the copies the create
    // input held.
    await configuring();
    void refreshed(c, peers).then(configuring, (error: unknown) =>
        peers.report("the copies were not read from the realm", error),
    );
    void secretsRefreshed(peers.reach).catch((error: unknown) =>
        peers.report("the realm's secret list was not read", error),
    );
    // Asking keeps the agent among the library's listeners, so only an
    // agent that reads its shelves asks.
    if (row.kind === "hand" || row.kind === "hire") {
        void shelvesRead(c, peers).catch((error: unknown) =>
            peers.report("the library's shelves were not read", error),
        );
    }
    if (
        (row.kind === "worker" || row.kind === "errand") &&
        row.brief !== undefined
    ) {
        const { kind, brief } = row;
        void briefTurn(c, options, peers, kind, brief, row.standingTask).catch(
            (error: unknown) =>
                peers.report(
                    "the realm was not told how the brief's turn ended",
                    error,
                ),
        );
    }
}

function oneAfterAnother(): Generation["inTurn"] {
    let last: Promise<unknown> = Promise.resolve();
    return (work) => {
        const done = last.then(work);
        last = done.catch(() => undefined);
        return done;
    };
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

const BY_SESSION: ReadonlySet<Submit["input"]["type"]> = new Set([
    "line",
    "notice",
    "worker_answer",
]);

/** Answers when the admission is committed, not when the turn ends. */
async function submit(c: AgentCall, message: Submit): Promise<void> {
    const { options, inTurn, receiving, peers } = generationOf(c);
    const admit = async () => {
        const id = await conversationFor(c, message.input);
        await withHarness(c, options, async (harness, context) => {
            const conversation = await harness.conversation(id, context);
            if (conversation === undefined) {
                throw new Error(`the agent has no conversation ${id}`);
            }
            const { input } = message;
            const admitted = (captions: string) => {
                const admission = admissionOf(message, captions);
                return conversation.submit(
                    admission.ended?.end.type === "aborted"
                        ? endedWithoutRun(admission, input.text)
                        : draftOf(
                              admission,
                              input.type === "line" ? "steer" : undefined,
                          ),
                    context,
                );
            };
            const files =
                input.type === "letter" || input.type === "question"
                    ? (input.files ?? [])
                    : [];
            if (files.length === 0) return admitted("");
            await receiving(async () => {
                // A repeat of a submit it took saves nothing again.
                const again =
                    (await harness.commit(
                        (tx) => tx.submissionByRequest(id, message.id),
                        context,
                    )) !== undefined;
                await admitted(
                    again ? "" : await received(peers, files, context),
                );
            });
        });
    };
    await (BY_SESSION.has(message.input.type) ? inTurn(admit) : admit());
}

function endedWithoutRun(admission: Admission, text: string): SubmissionDraft {
    return {
        type: "write",
        requestId: admission.id,
        entry: noticeEntry(text, admission.ended),
    };
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
                ended: {
                    call: message.call,
                    end: endOf(message.outcome),
                },
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
    const { registry, builtIns, peers, prompt } = generationOf(c);
    if (kept.changes === 0 || builtIns === undefined) return;
    const offer = offerOf(await rowAt(c));
    if (offer === undefined) return;
    builtIns.offer = offer;
    // Installing one again replaces it, so the next request offers what the
    // place says.
    for (const extension of builtIns.listed) {
        registry.install(offered(extension, offer));
    }
    if (offer.society !== undefined) {
        registry.install(society(peers, folderOf(prompt), offer.society));
    }
}

async function fill(c: AgentCall, message: Fill): Promise<void> {
    await generationOf(c).branch.fill(message.profile);
}

/** The profile the agent was filled from; absent before its fill. */
async function profileOf(
    c: Pick<AgentContext, "db">,
): Promise<ProfileId | undefined> {
    const row = await c.db.get<{ profile: ProfileId | null }>(
        "SELECT profile FROM agent WHERE id = 1",
    );
    return row?.profile ?? undefined;
}

async function moved(c: AgentCall, message: Moved): Promise<void> {
    if (message.character !== undefined) {
        const { version, text } = message.character;
        await keep(c, { character: { version, value: text } });
    }
    generationOf(c).branch.moved();
}

function remove_path(c: AgentCall, message: RemovePath): Promise<Removal> {
    return generationOf(c).branch.remove(message.path);
}

async function shelvesRead(c: AgentContext, peers: Peers): Promise<void> {
    const seen = await c.db.get<{ version: number }>(
        "SELECT version FROM copy WHERE name = 'shelves'",
    );
    const list = readShelves(await libraryCalls(peers).shelves(seen?.version));
    await keep(c, { shelves: { version: list.version, value: list.skills } });
}

async function shelves_changed(
    c: AgentCall,
    { list }: ShelvesChanged,
): Promise<void> {
    await keep(c, { shelves: { version: list.version, value: list.skills } });
}

async function copies(c: AgentCall, message: Copies): Promise<void> {
    await keep(c, message);
    await configured(c, generationOf(c).copies);
}

async function wakeTimers(c: AgentCall): Promise<void> {
    await withHarness(c, generationOf(c).options, () => resumeDueSleeps(c));
}
