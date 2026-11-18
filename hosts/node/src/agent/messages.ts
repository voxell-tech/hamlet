/**
 * The messages of `crates/hamlet_messages` that an agent takes or sends. A
 * field that is not set is absent, never `null`. An integer the agent
 * writes is a `u32`.
 */
import {
    arr,
    bigint,
    bool,
    compile,
    int,
    lit,
    num,
    obj,
    oneOf,
    opt,
    type Schema,
    type Static,
    str,
    union,
    unknown,
} from "@earendil-works/pi-schema";
import { ActorError, Address } from "../actor/actor/mod.ts";

declare const minted: unique symbol;

type Minted<Name extends string> = string & { readonly [minted]: Name };

/** The id a call out carries, which its taker finds a repeat by. */
export type CallId = Minted<"call">;

export type ConversationRef = Minted<"conversation">;

export type RealmTask = string;

export type AgentNumber = number;
export type LetterId = number;
export type ProfileId = number;
export type HamletId = string;
export type CommandId = number;

export const U32_MAX = 2 ** 32 - 1;

export function u32(value: number): number {
    if (Number.isNaN(value) || value <= 0) return 0;
    return Math.min(Math.round(value), U32_MAX);
}

/** The act did not run. `words` are for the model, or for a chief. */
export interface Refused<
    Why extends { readonly type: string } = { readonly type: string },
> {
    readonly type: "refused";
    readonly why: Why;
    readonly words: string;
}

export interface Stamped {
    readonly call: CallId;
    readonly conversation: ConversationRef;
    readonly realm_task?: RealmTask;
}

export type Program =
    | {
          readonly type: "script";
          readonly script: string;
          readonly network: boolean;
          readonly timeout_ms?: number;
      }
    | {
          readonly type: "tool";
          readonly name: string;
          /** A JSON object, as text. */
          readonly arguments: string;
      };

export interface Run extends Stamped {
    readonly program: Program;
}

export type Need = "network" | "command" | "edit";

export type Exit =
    | { readonly type: "code"; readonly code: number }
    | { readonly type: "timed_out" }
    | { readonly type: "failed"; readonly words: string }
    | { readonly type: "cut_off" }
    | { readonly type: "cancelled" };

export interface Cut {
    readonly total_bytes: number;
    readonly total_lines: number;
    readonly full_output: string;
}

export interface CommandEnd {
    readonly command: CommandId;
    readonly exit: Exit;
    readonly output: string;
    readonly cut?: Cut;
    readonly policy: string;
    readonly took_ms: number;
}

export type RunEnd =
    | ({ readonly type: "ran" } & CommandEnd)
    | {
          readonly type: "not_run";
          readonly why: { readonly type: string };
          readonly words: string;
      };

export type RunAnswer =
    | { readonly type: "ended"; readonly end: RunEnd }
    | { readonly type: "running"; readonly command: CommandId }
    | { readonly type: "waits_for_chief"; readonly needs: readonly Need[] }
    | Refused;

export interface ReadFile {
    readonly path: string;
    readonly offset?: number;
    readonly limit?: number;
}

export interface Search {
    readonly under: string;
    readonly pattern: string;
    readonly name?: string;
    readonly ignore_case: boolean;
}

export interface Find {
    readonly under: string;
    readonly name?: string;
}

export interface Note {
    readonly severity: "info" | "warn";
    readonly code: string;
    readonly message: string;
}

export interface Text {
    readonly text: string;
    readonly notes: readonly Note[];
    readonly truncation?: {
        readonly by?: "lines" | "bytes";
        readonly total_lines: number;
        readonly total_bytes: number;
        readonly output_lines: number;
        readonly output_bytes: number;
        readonly first_line_exceeds_limit: boolean;
    };
}

export type ReadAnswer =
    | ({ readonly type: "text" } & Text)
    | { readonly type: "image"; readonly mime: string; readonly data: string }
    | { readonly type: "failed"; readonly words: string }
    | Refused;

export type SearchAnswer =
    | ({ readonly type: "text" } & Text)
    | { readonly type: "failed"; readonly words: string }
    | Refused;

export interface WriteFile extends Stamped {
    readonly path: string;
    readonly content: string;
}

export interface Replace {
    readonly old: string;
    readonly new: string;
}

export interface EditFile extends Stamped {
    readonly path: string;
    readonly edits: readonly Replace[];
}

export interface Change {
    readonly path: string;
    readonly created: boolean;
    readonly hash: string;
    readonly diff?: {
        readonly diff: string;
        readonly patch: string;
        readonly first_changed_line?: number;
    };
}

export type ChangeEnd =
    | ({ readonly type: "changed" } & Change)
    | { readonly type: "failed"; readonly words: string }
    | {
          readonly type: "not_run";
          readonly why: { readonly type: string };
          readonly words: string;
      };

export type ChangeAnswer =
    | { readonly type: "ended"; readonly end: ChangeEnd }
    | { readonly type: "waits_for_chief"; readonly needs: readonly Need[] }
    | Refused;

export interface AddedTool {
    readonly name: string;
    readonly description: string;
    /** The JSON Schema of its arguments, as text. */
    readonly parameters: string;
}

export interface ToolList {
    readonly tools: readonly AddedTool[];
    readonly faults: readonly {
        readonly folder: string;
        readonly words: string;
    }[];
}

export type Which =
    | { readonly type: "all" }
    | { readonly type: "conversation"; readonly conversation: ConversationRef }
    | { readonly type: "one"; readonly call: CallId };

export interface Cancel {
    readonly which: Which;
}

export interface Cancelled {
    readonly commands: readonly CommandId[];
}

export interface WorkspaceActions {
    run(message: Run): RunAnswer;
    read_file(message: ReadFile): ReadAnswer;
    search(message: Search): SearchAnswer;
    find(message: Find): SearchAnswer;
    write_file(message: WriteFile): ChangeAnswer;
    edit_file(message: EditFile): ChangeAnswer;
    list_tools(): ToolList;
    cancel(message: Cancel): Cancelled;
}

/** `mayor` is the office of the caller's hamlet. */
export type Named =
    | { readonly type: "hand" }
    | { readonly type: "mayor" }
    | { readonly type: "mayor_of"; readonly hamlet: HamletId }
    | { readonly type: "agent"; readonly agent: AgentNumber };

export type Tier = "fifty" | "hundred" | "two_hundred" | "five_hundred";

export interface Tool<Act> {
    readonly call: CallId;
    /** The realm's task of the calling turn. */
    readonly task?: RealmTask;
    /** The calling turn's conversation. */
    readonly conversation?: ConversationRef;
    readonly act: Act;
}

export type Done = { readonly type: "done" } | Refused;

/** A society tool's act, or a built-in or hand tool's name. */
export interface ToolName {
    readonly name: string;
}

export type FoundAnswer =
    | {
          readonly type: "founded";
          readonly hamlet: HamletId;
          readonly mayor: AgentNumber;
      }
    | Refused;

export type HireAnswer =
    | { readonly type: "hired"; readonly agent: AgentNumber }
    | Refused;

export interface SocietyActs {
    found: {
        act: {
            readonly name: string;
            readonly mayor?: ProfileId;
            readonly tier: Tier;
            readonly purpose?: string;
        };
        answer: FoundAnswer;
    };
    hire: {
        act: {
            readonly hamlet?: HamletId;
            readonly under?: Named;
            readonly profile?: ProfileId;
            readonly purpose?: string;
        };
        answer: HireAnswer;
    };
    end: {
        act: { readonly hire: Named; readonly why?: string };
        answer: Done;
    };
    seat: { act: { readonly hire: Named }; answer: Done };
    tell: {
        act: { readonly to: Named; readonly text: string };
        answer: Done;
    };
    ask: {
        act: {
            readonly to: Named;
            readonly text: string;
            readonly conversation: ConversationRef;
        };
        answer: Done;
    };
    lay_road: {
        act: { readonly a: Named; readonly b: Named };
        answer: Done;
    };
    take_up_road: {
        act: { readonly a: Named; readonly b: Named };
        answer: Done;
    };
    request_road: {
        act: {
            readonly to: Named;
            readonly why?: string;
        };
        answer: Done;
    };
    set_tools: {
        act: {
            readonly hire: Named;
            readonly off?: readonly ToolName[];
            readonly on?: readonly ToolName[];
        };
        answer: Done;
    };
}

export type SocietyTool = keyof SocietyActs;

export interface Reply {
    readonly question: LetterId;
    readonly text: string;
}

export interface MaySpend {
    readonly task: RealmTask;
}

export interface Charge {
    readonly call: CallId;
    readonly task: RealmTask;
    readonly micros: number;
    readonly took_ms: number;
}

export interface OauthLogin {
    readonly access: string;
    readonly refresh: string;
    /** Whole seconds since the epoch. */
    readonly expires: number;
    readonly extra?: string;
}

export type Kept =
    | { readonly type: "key"; readonly key: string }
    | { readonly type: "oauth"; readonly login: OauthLogin };

export type CredentialAnswer =
    | Kept
    | Refused<{ readonly type: "not_signed_in"; readonly provider: string }>;

export interface KeepCredential {
    readonly provider: string;
    readonly value: Kept;
}

export interface TurnFailed {
    readonly task: RealmTask;
    readonly conversation: ConversationRef;
    readonly words: string;
    readonly cause: "model" | "budget";
}

export type LapseReason = "stopped" | "rewound" | "ended" | "resumed";

export interface Lapse {
    /** Absent: every conversation of the agent. */
    readonly conversation?: ConversationRef;
    readonly why: LapseReason;
}

export interface OpenTask {
    /** A repeat carries the same id. */
    readonly request: string;
    readonly turn: "retry" | "reflection";
}

export type TaskOpened =
    | { readonly type: "opened"; readonly task: RealmTask }
    | Refused;

/** The versions the agent holds; the realm answers the parts newer than these. */
export interface ReadCopies {
    readonly charter?: number;
    readonly character?: number;
    readonly model?: number;
    readonly settings?: number;
}

/** The realm's secrets in force, which only a scrub reads. */
export interface SecretList {
    readonly version: number;
    readonly values: readonly string[];
}

export type Ended =
    | { readonly type: "answered"; readonly text: string }
    | { readonly type: "failed"; readonly why: string };

export interface TurnEnded {
    readonly ended: Ended;
}

export type RealmActions = {
    readonly [N in SocietyTool]: (
        message: Tool<SocietyActs[N]["act"]>,
    ) => SocietyActs[N]["answer"];
} & {
    reply(message: Reply): Done;
    may_spend(message: MaySpend): Done;
    charge(message: Charge): null;
    credential(message: { readonly provider: string }): CredentialAnswer;
    keep_credential(message: KeepCredential): null;
    turn_failed(message: TurnFailed): null;
    lapse(message: Lapse): null;
    open_task(message: OpenTask): TaskOpened;
    read_copies(message: ReadCopies): Copies;
    secrets(): SecretList;
    worker_done(message: TurnEnded): null;
    errand_done(message: TurnEnded): null;
};

export interface Author {
    readonly agent: AgentNumber;
    readonly name?: string;
}

export interface Attached {
    readonly id: string;
    readonly at?: number;
    readonly name?: string;
    readonly size?: number;
}

export type Input =
    | {
          readonly type: "line";
          readonly text: string;
          readonly images?: readonly Attached[];
          readonly files?: readonly Attached[];
          /** The note of the lead who reviewed the line. */
          readonly note?: string;
      }
    /** A chief's ruling, or news the realm tells the agent. */
    | {
          readonly type: "notice";
          readonly text: string;
          readonly conversation?: string;
      }
    | {
          readonly type: "worker_answer";
          readonly worker: AgentNumber;
          readonly text: string;
      }
    | { readonly type: "letter"; readonly from: Author; readonly text: string }
    | {
          readonly type: "question";
          readonly letter: LetterId;
          readonly from: Author;
          readonly text: string;
      }
    | {
          readonly type: "answer";
          readonly question: LetterId;
          readonly from: Author;
          readonly text: string;
          readonly conversation: string;
      };

export interface Submit {
    /** A second `submit` with the same id is the first one. */
    readonly id: string;
    readonly task?: RealmTask;
    readonly input: Input;
}

export type Outcome =
    | { readonly type: "run"; readonly end: RunEnd }
    | { readonly type: "change"; readonly end: ChangeEnd };

export interface Finished {
    readonly call: string;
    readonly outcome: Outcome;
}

export interface Place {
    /** A `u64`: a number while it is a safe integer, else a bigint. */
    readonly version: number | bigint;
    readonly lead?: AgentNumber;
    readonly hires: readonly AgentNumber[];
    readonly roads: readonly AgentNumber[];
    readonly tools_off: readonly ToolName[];
    readonly folder?: string;
}

export interface Fill {
    readonly profile: ProfileId;
    readonly branch: string;
}

export interface AgentInput {
    readonly kind: "hand" | "hire" | "worker" | "errand";
    /** The standing task: it pays for a turn whose input names none. */
    readonly task: RealmTask;
    readonly place: Place;
    readonly brief?: Brief;
    /** Every part the realm holds for the agent, at its version. */
    readonly copies: Copies;
}

/** The one turn a worker or an errand runs when it is created. */
export interface Brief {
    /** An errand's whole prompt; absent for a worker. */
    readonly instructions?: string;
    readonly text: string;
    readonly tools?: readonly { readonly name: string }[];
    readonly model?: string;
}

export interface Versioned<T> {
    readonly version: number;
    readonly value: T;
}

export type Effort =
    | "off"
    | "minimal"
    | "low"
    | "medium"
    | "high"
    | "xhigh"
    | "max";

/** Dollars per million tokens. */
export interface Price {
    readonly input: number;
    readonly output: number;
    readonly cache_read: number;
    readonly cache_write: number;
}

export interface ModelChoice {
    /** `provider::model`, as the catalogue names it. */
    readonly model: string;
    readonly effort?: Effort;
    readonly fast?: boolean;
    readonly price?: Price;
}

export interface AgentSettings {
    readonly reflect_every: number;
    readonly allow_net: boolean;
}

/** The parts the realm owns that the agent keeps a copy of. */
export interface Copies {
    readonly charter?: Versioned<string>;
    readonly character?: Versioned<string>;
    readonly model?: Versioned<ModelChoice>;
    readonly settings?: Versioned<AgentSettings>;
}

export interface AgentActions {
    submit(message: Submit): null;
    finished(message: Finished): null;
    place(message: Place): null;
    fill(message: Fill): null;
    copies(message: Copies): null;
}

export type RequestId = string;

/** A sum that can pass 2^32, written as decimal text. */
export type Count = string;

export interface ChatRef {
    readonly with: AgentNumber;
    readonly from?: AgentNumber;
    readonly kind?: "letters" | "questions";
    readonly past?: number;
}

export type ItemKind =
    | "user"
    | "assistant"
    | "reasoning"
    | "tool"
    | "notice"
    | "error"
    | "answered";

export interface ChatImage {
    readonly at?: number;
    readonly id: string;
}

export interface ChatFile {
    readonly at?: number;
    readonly id: string;
    readonly name: string;
    readonly size: number;
}

export interface ToolChange {
    readonly path: string;
    readonly created: boolean;
    readonly added: number;
    readonly removed: number;
    readonly total: number;
    readonly count: string;
    readonly lines: readonly {
        readonly mark: "same" | "added" | "removed";
        readonly text: string;
    }[];
}

export interface ChatTool {
    readonly name: string;
    readonly summary: string;
    /** The exit code's 32 bits: a negative code `c` is `c >>> 0`. */
    readonly exit?: number;
    readonly ms?: number;
    readonly failure?: string;
    readonly change?: ToolChange;
}

export interface ChatItem {
    readonly id: string;
    /** Whole seconds since the epoch. */
    readonly at: number;
    readonly kind: ItemKind;
    readonly text: string;
    readonly author?: string;
    readonly images?: readonly ChatImage[];
    readonly files?: readonly ChatFile[];
    readonly rewindable?: boolean;
    readonly tool?: ChatTool;
}

export interface Usage {
    readonly calls: Count;
    readonly unreported: Count;
    readonly input: Count;
    readonly cached: Count;
    readonly output: Count;
    readonly reasoning: Count;
    readonly ms: Count;
    /** Millionths of a dollar. */
    readonly micros: Count;
    readonly last_input: Count;
    readonly summary: string;
}

export interface ChatPage {
    readonly epoch: number;
    readonly number: number;
    readonly session?: number;
    readonly items: readonly ChatItem[];
    readonly earlier?: string;
    readonly working: boolean;
    readonly pending: number;
    readonly usage: Usage;
}

export type ChatOp =
    | {
          readonly type: "add";
          readonly item: ChatItem;
          readonly replaces?: string;
      }
    | {
          readonly type: "append";
          readonly item: string;
          readonly at: number;
          readonly text: string;
      }
    | {
          readonly type: "status";
          readonly working: boolean;
          readonly pending: number;
      }
    | { readonly type: "usage"; readonly usage: Usage }
    | { readonly type: "reset" };

export interface ChatEvent {
    readonly chat: ChatRef;
    readonly epoch: number;
    readonly number: number;
    readonly ops: readonly ChatOp[];
}

export interface Session {
    readonly number: number;
    readonly began: number;
    readonly modified: number;
    readonly current: boolean;
    readonly messages: number;
    readonly tokens: Count;
    readonly title: string;
}

export interface Stop {
    readonly agent?: AgentNumber;
    readonly id: RequestId;
}

export interface NewSession {
    readonly id: RequestId;
    readonly to: AgentNumber;
}

export interface Resume {
    readonly id: RequestId;
    /** A bigint past 2^53, which names no session but is worded as asked. */
    readonly number: number | bigint;
    readonly to: AgentNumber;
}

export interface Retry {
    readonly id: RequestId;
    readonly to: AgentNumber;
}

export interface Rewind {
    readonly chat: ChatRef;
    readonly id: RequestId;
    readonly item: string;
}

export interface Open {
    readonly before?: string;
    readonly chat: ChatRef;
    readonly id: RequestId;
}

export type Login =
    | {
          readonly id: RequestId;
          readonly start: {
              readonly method: "key" | "subscription";
              readonly provider: string;
          };
      }
    | {
          readonly id: RequestId;
          readonly reply: { readonly login: string; readonly value: string };
      }
    | { readonly id: RequestId; readonly cancel: string };

export type LoginStep =
    | { readonly url: string; readonly instructions?: string }
    | { readonly code: string }
    | { readonly prompt: string; readonly secret: boolean }
    | { readonly progress: string }
    | { readonly done: string }
    | { readonly failed: string };

export type LoginAnswer =
    | {
          readonly type: "step";
          readonly login: string;
          readonly step: LoginStep;
      }
    | { readonly type: "done"; readonly login: string }
    | {
          readonly type: "failed";
          readonly login: string;
          readonly words: string;
      }
    /** The agent is not the Hand, which alone signs in. */
    | { readonly type: "not_the_hand" };

export type ResumeAnswer =
    | { readonly type: "back"; readonly number: number }
    | Refused<{ readonly type: "no_such_session"; readonly number: number }>;

export type RetryAnswer =
    | { readonly type: "done" }
    | Refused<
          | { readonly type: "running" }
          | { readonly type: "answered" }
          | { readonly type: "no_turn" }
      >;

export type RewindAnswer =
    | {
          readonly type: "rewound";
          readonly text: string;
          readonly images?: readonly ChatImage[];
          readonly files?: readonly ChatFile[];
      }
    | Refused<
          | { readonly type: "not_a_chief_line" }
          | { readonly type: "no_such_item"; readonly item: string }
          | { readonly type: "not_current" }
          | { readonly type: "working" }
      >;

/** A model the listing holds, named as the catalogue names it. */
export interface ListedModel {
    readonly name: string;
    readonly title: string;
    readonly context: number;
    readonly price?: Price;
}

export interface ModelList {
    readonly models: readonly ListedModel[];
}

export interface Check {
    readonly model: string;
}

/** As today's check answers: the model's hello, its failure, or a refusal of the name. */
export interface Checked {
    readonly type: "answered" | "failed" | "refused";
    readonly words: string;
}

/** A client's calls of an agent, each answered with a value. */
export interface ClientActions {
    stop(message: Stop): { readonly turns: number };
    new_session(message: NewSession): { readonly number: number };
    resume(message: Resume): ResumeAnswer;
    retry(message: Retry): RetryAnswer;
    rewind(message: Rewind): RewindAnswer;
    read_chat(message: Open): ChatPage;
    read_history(): { readonly sessions: readonly Session[] };
    read_prompt(): { readonly text: string };
    reflect(): "asked" | "already";
    login(message: Login): LoginAnswer;
}

export const WORKSPACE: Address<WorkspaceActions> = Address.of(
    "workspace",
    "local",
);
export const REALM: Address<RealmActions> = Address.of("realm", "local");

/** As each message's `Message::From` says. */
export const SENDERS = {
    submit: REALM,
    finished: WORKSPACE,
    place: REALM,
    fill: REALM,
    copies: REALM,
} as const satisfies { readonly [N in keyof AgentActions]: Address };

const OPEN = { open: true } as const;
const U32 = int({ minimum: 0, maximum: U32_MAX });
const AUTHOR = obj({ agent: U32, name: opt(str()) }, OPEN);
const PLACE = obj(
    {
        version: union([int(), bigint()]),
        lead: opt(U32),
        hires: arr(U32),
        roads: arr(U32),
        tools_off: arr(obj({ name: str() }, OPEN)),
        folder: opt(str()),
    },
    OPEN,
);
const MODEL = obj(
    {
        model: str(),
        effort: opt(
            oneOf(["off", "minimal", "low", "medium", "high", "xhigh", "max"]),
        ),
        fast: opt(bool()),
        price: opt(
            obj(
                {
                    input: num(),
                    output: num(),
                    cache_read: num(),
                    cache_write: num(),
                },
                OPEN,
            ),
        ),
    },
    OPEN,
);
const BRIEF = obj(
    {
        instructions: opt(str()),
        text: str(),
        tools: opt(arr(obj({ name: str() }, OPEN))),
        model: opt(str()),
    },
    OPEN,
);
const SETTINGS = obj({ reflect_every: U32, allow_net: bool() }, OPEN);

function versioned<S extends Schema>(value: S) {
    return obj({ version: U32, value }, OPEN);
}

const COPIES_SHAPE = obj(
    {
        charter: opt(versioned(str())),
        character: opt(versioned(str())),
        model: opt(versioned(MODEL)),
        settings: opt(versioned(SETTINGS)),
    },
    OPEN,
);

const INPUT = union([
    obj(
        {
            type: lit("line"),
            text: str(),
            images: opt(arr(unknown())),
            files: opt(arr(unknown())),
            note: opt(str()),
        },
        OPEN,
    ),
    obj({ type: lit("notice"), text: str(), conversation: opt(str()) }, OPEN),
    obj({ type: lit("worker_answer"), worker: U32, text: str() }, OPEN),
    obj({ type: lit("letter"), from: AUTHOR, text: str() }, OPEN),
    obj(
        { type: lit("question"), letter: U32, from: AUTHOR, text: str() },
        OPEN,
    ),
    obj(
        {
            type: lit("answer"),
            question: U32,
            from: AUTHOR,
            text: str(),
            conversation: str(),
        },
        OPEN,
    ),
]);

/** Each drops `null` fields, ignores unknown ones and checks the rest. */
export const read: {
    readonly [N in keyof AgentActions]: (
        raw: unknown,
    ) => Parameters<AgentActions[N]>[0];
} & { readonly agentInput: (raw: unknown) => AgentInput } = {
    agentInput: reader(
        obj(
            {
                kind: oneOf(["hand", "hire", "worker", "errand"]),
                task: str(),
                place: PLACE,
                brief: opt(BRIEF),
                copies: COPIES_SHAPE,
            },
            OPEN,
        ),
        ({ kind, task, place, brief, copies }) => ({
            kind,
            task,
            place: placeOf(place),
            ...(brief === undefined ? {} : { brief: briefOf(brief) }),
            copies: copiesOf(copies),
        }),
    ),
    submit: reader(
        obj({ id: str(), task: opt(str()), input: INPUT }, OPEN),
        (submit) => submit as Submit,
    ),
    // The end's own fields are the workspace's; only its tag is checked here.
    finished: reader(
        obj(
            {
                call: str(),
                outcome: obj(
                    {
                        type: oneOf(["run", "change"]),
                        end: obj({ type: str() }, OPEN),
                    },
                    OPEN,
                ),
            },
            OPEN,
        ),
        (finished) => finished as unknown as Finished,
    ),
    place: reader(PLACE, placeOf),
    fill: reader(obj({ profile: U32, branch: str() }, OPEN), (fill) => fill),
    copies: reader(COPIES_SHAPE, copiesOf),
};

/**
 * A client's number is a `u64` or an `i64`. One that is no safe integer
 * names nothing the agent has, so it reads as `Infinity`.
 */
const WIDE = union([int(), bigint()]);
const CHAT_REF = obj(
    {
        with: WIDE,
        from: opt(WIDE),
        kind: opt(oneOf(["letters", "questions"])),
        past: opt(WIDE),
    },
    OPEN,
);
const LOGIN = union([
    obj(
        {
            id: str(),
            start: obj(
                { method: oneOf(["key", "subscription"]), provider: str() },
                OPEN,
            ),
        },
        OPEN,
    ),
    obj({ id: str(), reply: obj({ login: str(), value: str() }, OPEN) }, OPEN),
    obj({ id: str(), cancel: str() }, OPEN),
]);

/** Each reads a client call's arguments as `read` reads a message. */
export const readClient: {
    readonly [N in keyof ClientActions]: (
        raw: readonly unknown[],
    ) => Parameters<ClientActions[N]>;
} = {
    stop: first(
        obj({ agent: opt(WIDE), id: str() }, OPEN),
        ({ agent, id }) => ({
            id,
            ...(agent === undefined ? {} : { agent: wide(agent) }),
        }),
    ),
    new_session: first(obj({ id: str(), to: WIDE }, OPEN), ({ id, to }) => ({
        id,
        to: wide(to),
    })),
    resume: first(
        obj({ id: str(), number: WIDE, to: WIDE }, OPEN),
        ({ id, number, to }) => ({ id, number, to: wide(to) }),
    ),
    retry: first(obj({ id: str(), to: WIDE }, OPEN), ({ id, to }) => ({
        id,
        to: wide(to),
    })),
    rewind: first(
        obj({ chat: CHAT_REF, id: str(), item: str() }, OPEN),
        ({ chat, id, item }) => ({ chat: chatRefOf(chat), id, item }),
    ),
    read_chat: first(
        obj({ before: opt(str()), chat: CHAT_REF, id: str() }, OPEN),
        ({ before, chat, id }) => ({
            id,
            chat: chatRefOf(chat),
            ...(before === undefined ? {} : { before }),
        }),
    ),
    read_history: () => [],
    read_prompt: () => [],
    reflect: () => [],
    login: first(LOGIN, (login) => login as Login),
};

function first<S extends Schema, T>(
    schema: S,
    shape: (value: Static<S>) => T,
): (raw: readonly unknown[]) => [T] {
    const one = reader(schema, shape);
    return (raw) => [one(raw[0])];
}

function wide(value: number | bigint): number {
    return typeof value === "bigint" ? Number.POSITIVE_INFINITY : value;
}

function chatRefOf({
    with: agent,
    from,
    kind,
    past,
}: Static<typeof CHAT_REF>): ChatRef {
    return {
        with: wide(agent),
        ...(from === undefined ? {} : { from: wide(from) }),
        ...(kind === undefined ? {} : { kind }),
        ...(past === undefined ? {} : { past: wide(past) }),
    };
}

/** Each part as the agent keeps it, without the fields a newer realm adds. */
function copiesOf(raw: Static<typeof COPIES_SHAPE>): Copies {
    const part = <T, V>(
        given: { version: number; value: T } | undefined,
        value: (from: T) => V,
    ): { version: number; value: V } | undefined =>
        given === undefined
            ? undefined
            : { version: given.version, value: value(given.value) };
    const model = part(raw.model, ({ model, effort, fast, price }) => ({
        model,
        ...(effort === undefined ? {} : { effort }),
        ...(fast === undefined ? {} : { fast }),
        ...(price === undefined
            ? {}
            : {
                  price: {
                      input: price.input,
                      output: price.output,
                      cache_read: price.cache_read,
                      cache_write: price.cache_write,
                  },
              }),
    }));
    const settings = part(raw.settings, ({ reflect_every, allow_net }) => ({
        reflect_every,
        allow_net,
    }));
    return {
        ...(raw.charter === undefined ? {} : { charter: raw.charter }),
        ...(raw.character === undefined ? {} : { character: raw.character }),
        ...(model === undefined ? {} : { model }),
        ...(settings === undefined ? {} : { settings }),
    };
}

function reader<S extends Schema, T>(
    schema: S,
    shape: (value: Static<S>) => T,
): (raw: unknown) => T {
    const validator = compile(schema);
    return (raw) => {
        const read = validator.validate(withoutNulls(raw));
        if (!read.ok) {
            throw new ActorError(
                "message",
                "unreadable",
                `the message cannot be read: ${read.errors[0]?.message ?? ""}`,
            );
        }
        return shape(read.value as Static<S>);
    };
}

function withoutNulls(value: unknown): unknown {
    if (Array.isArray(value)) return value.map(withoutNulls);
    if (value === null || typeof value !== "object") return value;
    return Object.fromEntries(
        Object.entries(value)
            .filter(([, field]) => field !== null)
            .map(([key, field]) => [key, withoutNulls(field)]),
    );
}

/**
 * A newer realm may add fields, which are dropped: the row keeps the place,
 * and a field it cannot serialise, such as a bigint, would fail the write.
 */
function placeOf({
    version,
    lead,
    hires,
    roads,
    tools_off,
    folder,
}: Static<typeof PLACE>): Place {
    return {
        version,
        ...(lead === undefined ? {} : { lead }),
        hires,
        roads,
        tools_off: tools_off.map(({ name }) => ({ name })),
        ...(folder === undefined ? {} : { folder }),
    };
}

function briefOf({
    instructions,
    text,
    tools,
    model,
}: Static<typeof BRIEF>): Brief {
    return {
        ...(instructions === undefined ? {} : { instructions }),
        text,
        ...(tools === undefined
            ? {}
            : { tools: tools.map(({ name }) => ({ name })) }),
        ...(model === undefined ? {} : { model }),
    };
}
