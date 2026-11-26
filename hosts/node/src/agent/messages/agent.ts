/**
 * The agent's own messages, which the realm, its workspace, its profile,
 * the library and a client send it, and how the agent reads them.
 */
import {
    arr,
    bigint,
    bool,
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
import type { Address } from "../../actor/actor/mod.ts";
import { LIBRARY, SHELF_LIST, type ShelfList, shelfListOf } from "./library.ts";
import {
    CHARACTER,
    type Character,
    type CommitId,
    characterOf,
    OWN_PROFILE,
    PIECE,
    type Piece,
    pieceOf,
} from "./profile.ts";
import { REALM, type ToolName } from "./realm.ts";
import {
    type AgentNumber,
    type CallId,
    type LetterId,
    OPEN,
    type ProfileId,
    type RealmTask,
    type Refused,
    reader,
    U32,
} from "./shared.ts";
import { type ChangeEnd, type RunEnd, WORKSPACE } from "./workspace.ts";

export interface Author {
    readonly agent: AgentNumber;
    readonly name?: string;
}

/** A file of a parcel, as its receiver is told of it. */
export interface ParcelFile {
    readonly parcel: number;
    /** The file's place in the parcel, from 0. */
    readonly file: number;
    /** Where the receiver keeps it, under its `inbox/`. */
    readonly path: string;
    /** A `u64`: a number while it is a safe integer, else a bigint. */
    readonly size: number | bigint;
}

/** An image of a chief's line, kept under `inbox/images/` and given to the model. */
export interface LineImage {
    readonly image: { readonly id: string; readonly at?: number };
    readonly file: ParcelFile;
}

/** A file of a chief's line, kept under `inbox/`. */
export interface LineFile {
    readonly attached: {
        readonly id: string;
        readonly at?: number;
        readonly name: string;
        readonly size: number;
    };
    readonly file: ParcelFile;
}

export type Input =
    | {
          readonly type: "line";
          readonly text: string;
          readonly images?: readonly LineImage[];
          readonly files?: readonly LineFile[];
          /** The note of the lead who reviewed the line. */
          readonly note?: string;
          /** The name of the chief who said it. */
          readonly chief?: string;
      }
    /** A chief's ruling, or news the realm tells the agent. */
    | {
          readonly type: "notice";
          readonly text: string;
          readonly conversation?: string;
          readonly held?: HeldCall;
      }
    | {
          readonly type: "worker_answer";
          readonly from: Author;
          readonly text: string;
          readonly conversation?: string;
      }
    | {
          readonly type: "letter";
          readonly from: Author;
          readonly text: string;
          readonly files?: readonly ParcelFile[];
      }
    | {
          readonly type: "question";
          readonly letter: LetterId;
          readonly from: Author;
          readonly text: string;
          readonly files?: readonly ParcelFile[];
      }
    | {
          readonly type: "answer";
          readonly question: LetterId;
          readonly from: Author;
          readonly text: string;
          readonly conversation: string;
      };

export interface PromptSection {
    readonly name: string;
    readonly text: string;
}

/** A society call the realm kept for a chief, which a ruling or a stop ends. */
export interface HeldCall {
    readonly call: CallId;
    readonly end: "ran" | "blocked" | "aborted" | "withdrawn";
}

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
    /** Whether its hamlet runs in manual mode. */
    readonly manual: boolean;
    /** Whether it is a mayor while another hamlet lives, so it may agree a road with another hamlet's mayor. */
    readonly agrees_roads: boolean;
}

export interface Fill {
    readonly profile: ProfileId;
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
    /** Absent is every built-in for a worker, and none for an errand. */
    readonly tools?: readonly { readonly name: string }[];
    readonly model?: string;
}

export interface Versioned<T> {
    readonly version: number;
    readonly value: T;
}

/** Every effort, weakest first. */
export const EFFORTS = [
    "off",
    "minimal",
    "low",
    "medium",
    "high",
    "xhigh",
    "max",
] as const;

export type Effort = (typeof EFFORTS)[number];

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

/** A model server the realm's settings name, serving `name::model`. */
export interface Endpoint {
    readonly name: string;
    readonly base_url: string;
}

export interface AgentSettings {
    readonly reflect_every: number;
    readonly allow_net: boolean;
    readonly endpoints?: readonly Endpoint[];
}

/** The parts the realm owns that the agent keeps a copy of. */
export interface Copies {
    readonly charter?: Versioned<string>;
    readonly model?: Versioned<ModelChoice>;
    readonly settings?: Versioned<AgentSettings>;
}

/** The agent's profile moved its branch to `tip`, or changed the character there. */
export interface Moved {
    readonly tip: CommitId;
    readonly character?: Character;
}

/** Asks the agent to remove a file or a folder of its filesystem, for a purge. */
export interface RemovePath {
    readonly path: string;
}

/** `later`: a turn or other work of the agent runs, and it removes the path once that ends. */
export type Removal = "removed" | "later";

export interface ShelvesChanged {
    readonly list: ShelfList;
}

/** An ended hire's agent stopped its turn and saved its files to its branch, whose tip is `tip`. */
export interface Retired {
    readonly tip: CommitId;
}

/** Where the next part of an inbox starts: at the first file whose path is `path` or after it, from its byte `offset`. */
export interface InboxCursor {
    readonly path: string;
    readonly offset: number;
}

/** Reads one part of the files in the agent's `inbox/`, in path order from `after`. */
export interface ReadInbox {
    readonly after?: InboxCursor;
}

/** Each piece's path is from `inbox/`. */
export interface InboxPart {
    readonly files: readonly Piece[];
    /** Absent on the last part. */
    readonly next?: InboxCursor;
}

/** One part of the inbox of the hire `from`, merged into this agent's, to write under `inbox/from-hire-<from>/`. */
export interface TakeInbox {
    readonly from: AgentNumber;
    readonly files: readonly Piece[];
}

/** Paths from `inbox/` to remove, once the hire this one was merged into took them. */
export interface DropInbox {
    readonly paths: readonly string[];
}

/** Whether an input under one of `tasks` is still to end, or one ended less than `within_ms` ago. */
export interface OnTask {
    readonly tasks: readonly RealmTask[];
    readonly within_ms: number | bigint;
}

export interface AgentActions {
    submit(message: Submit): null;
    finished(message: Finished): null;
    place(message: Place): null;
    fill(message: Fill): null;
    copies(message: Copies): null;
    moved(message: Moved): null;
    remove_path(message: RemovePath): Removal;
    shelves_changed(message: ShelvesChanged): null;
    stop(message: Stop): { readonly turns: number };
    retire(message: null): Retired;
    /** The agent answers, then ends itself for good. */
    erase(message: null): null;
    read_inbox(message: ReadInbox): InboxPart;
    /** It gives no turn. */
    take_inbox(message: TakeInbox): null;
    drop_inbox(message: DropInbox): null;
    on_task(message: OnTask): boolean;
}

/** As each message's `Message::From` says. */
export const SENDERS = {
    submit: REALM,
    finished: WORKSPACE,
    place: REALM,
    fill: REALM,
    copies: REALM,
    moved: OWN_PROFILE,
    remove_path: OWN_PROFILE,
    shelves_changed: LIBRARY,
    stop: REALM,
    retire: REALM,
    erase: REALM,
    read_inbox: REALM,
    take_inbox: REALM,
    drop_inbox: REALM,
    on_task: REALM,
} as const satisfies {
    readonly [N in keyof AgentActions]: Address | typeof OWN_PROFILE;
};

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
    /** Milliseconds since the epoch. */
    readonly at: Count;
    readonly kind: ItemKind;
    readonly text: string;
    readonly author?: string;
    /** The name of the chief who said a chief's line. */
    readonly chief?: string;
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

/** The realm's stop of the agent; a repeat of its number is the first. */
export interface Stop {
    /** Below 2^53, so it is read whole even when it comes as a bigint. */
    readonly number: number;
    readonly withdrawn?: readonly Withdrawn[];
}

/** A chief's line the realm withdrew before the agent took it. */
export interface Withdrawn {
    readonly id: string;
    readonly line: Input;
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
    | Refused<{ readonly type: "running" } | { readonly type: "answered" }>;

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
    /** The most images one line may give it; absent when it takes none. */
    readonly images?: number;
    /** The efforts it can be asked for, weakest first. */
    readonly efforts?: readonly Effort[];
    /** Whether its provider has a fast tier it can be asked for. */
    readonly fast?: boolean;
}

export interface ModelList {
    readonly models: readonly ListedModel[];
}

export interface Check {
    /** The model the agent runs when absent. */
    readonly model?: string;
}

/** The model's hello, its failure, or a refusal of its name. */
export interface ProviderView {
    readonly configured: boolean;
    readonly id: string;
    readonly label: string;
    readonly methods: readonly ("key" | "subscription")[];
    readonly name: string;
}

export interface ProviderList {
    readonly providers: readonly ProviderView[];
}

/** What the agent knows, as a client reads it. */
export interface Papers {
    readonly character: string;
    readonly learned: readonly string[];
    readonly memory: readonly {
        readonly description: string;
        readonly path: string;
        readonly text: string;
    }[];
    readonly skills: readonly {
        readonly description: string;
        readonly name: string;
    }[];
}

export interface EditMemory {
    /** A path under `memory/`, starting with it. */
    readonly path: string;
    readonly text: string;
}

export interface Checked {
    readonly type: "answered" | "failed" | "refused";
    readonly words: string;
    /** The models its endpoint lists, after a connection check. */
    readonly models?: number;
}

/** A client's calls of an agent, each answered with a value. */
export interface ClientActions {
    new_session(message: NewSession): { readonly number: number };
    resume(message: Resume): ResumeAnswer;
    retry(message: Retry): RetryAnswer;
    rewind(message: Rewind): RewindAnswer;
    read_chat(message: Open): ChatPage;
    read_history(): { readonly sessions: readonly Session[] };
    read_prompt(): { readonly sections: readonly PromptSection[] };
    reflect(): "asked" | "already";
    login(message: Login): LoginAnswer;
    read_models(): ModelList;
    images_taken(): { readonly per_line?: number };
    check(message: Check): Checked;
    check_connection(): Checked;
    read_providers(): ProviderList;
    read_papers(): Papers;
    edit_memory(message: EditMemory): null;
}

const AUTHOR = obj({ agent: U32, name: opt(str()) }, OPEN);

/** A `u64`: a number while it is a safe integer, else a bigint. */
const U64 = union([int(), bigint()]);

const PLACE = obj(
    {
        version: U64,
        lead: opt(U32),
        hires: arr(U32),
        roads: arr(U32),
        tools_off: arr(obj({ name: str() }, OPEN)),
        folder: opt(str()),
        manual: opt(bool()),
        agrees_roads: opt(bool()),
    },
    OPEN,
);

const PARCEL_FILE = obj(
    {
        parcel: U32,
        file: U32,
        path: str(),
        size: U64,
    },
    OPEN,
);

const INBOX_CURSOR = obj({ path: str(), offset: U32 }, OPEN);

const MODEL = obj(
    {
        model: str(),
        effort: opt(oneOf(EFFORTS)),
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

const SETTINGS = obj(
    {
        reflect_every: U32,
        allow_net: bool(),
        endpoints: opt(arr(obj({ name: str(), base_url: str() }, OPEN))),
    },
    OPEN,
);

function versioned<S extends Schema>(value: S) {
    return obj({ version: U32, value }, OPEN);
}

const COPIES_SHAPE = obj(
    {
        charter: opt(versioned(str())),
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
            images: opt(
                arr(
                    obj(
                        {
                            image: obj({ id: str(), at: opt(int()) }, OPEN),
                            file: PARCEL_FILE,
                        },
                        OPEN,
                    ),
                ),
            ),
            files: opt(
                arr(
                    obj(
                        {
                            attached: obj(
                                {
                                    id: str(),
                                    at: opt(int()),
                                    name: str(),
                                    size: int(),
                                },
                                OPEN,
                            ),
                            file: PARCEL_FILE,
                        },
                        OPEN,
                    ),
                ),
            ),
            note: opt(str()),
            chief: opt(str()),
        },
        OPEN,
    ),
    obj(
        {
            type: lit("notice"),
            text: str(),
            conversation: opt(str()),
            held: opt(
                obj(
                    {
                        call: str(),
                        end: oneOf(["ran", "blocked", "aborted", "withdrawn"]),
                    },
                    OPEN,
                ),
            ),
        },
        OPEN,
    ),
    obj(
        {
            type: lit("worker_answer"),
            from: AUTHOR,
            text: str(),
            conversation: opt(str()),
        },
        OPEN,
    ),
    obj(
        {
            type: lit("letter"),
            from: AUTHOR,
            text: str(),
            files: opt(arr(PARCEL_FILE)),
        },
        OPEN,
    ),
    obj(
        {
            type: lit("question"),
            letter: U32,
            from: AUTHOR,
            text: str(),
            files: opt(arr(PARCEL_FILE)),
        },
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
    fill: reader(obj({ profile: U32 }, OPEN), ({ profile }) => ({ profile })),
    copies: reader(COPIES_SHAPE, copiesOf),
    moved: reader(
        obj({ tip: str(), character: opt(CHARACTER) }, OPEN),
        ({ tip, character }) => ({
            tip,
            ...(character === undefined
                ? {}
                : { character: characterOf(character) }),
        }),
    ),
    remove_path: reader(obj({ path: str() }, OPEN), ({ path }) => ({ path })),
    shelves_changed: reader(obj({ list: SHELF_LIST }, OPEN), ({ list }) => ({
        list: shelfListOf(list),
    })),
    stop: reader(
        obj(
            {
                number: union([int(), bigint()]),
                withdrawn: opt(arr(obj({ id: str(), line: INPUT }, OPEN))),
            },
            OPEN,
        ),
        ({ number, withdrawn }) => ({
            number: Number(number),
            ...(withdrawn === undefined
                ? {}
                : { withdrawn: withdrawn as Withdrawn[] }),
        }),
    ),
    retire: reader(unknown(), () => null),
    erase: reader(unknown(), () => null),
    read_inbox: reader(obj({ after: opt(INBOX_CURSOR) }, OPEN), ({ after }) =>
        after === undefined
            ? {}
            : { after: { path: after.path, offset: after.offset } },
    ),
    take_inbox: reader(
        obj({ from: U32, files: arr(PIECE) }, OPEN),
        ({ from, files }) => ({ from, files: files.map(pieceOf) }),
    ),
    drop_inbox: reader(obj({ paths: arr(str()) }, OPEN), ({ paths }) => ({
        paths,
    })),
    on_task: reader(
        obj({ tasks: arr(str()), within_ms: U64 }, OPEN),
        ({ tasks, within_ms }) => ({ tasks, within_ms }),
    ),
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
    read_models: () => [],
    images_taken: () => [],
    check: first(obj({ model: opt(str()) }, OPEN), ({ model }) =>
        model === undefined ? {} : { model },
    ),
    check_connection: () => [],
    read_providers: () => [],
    read_papers: () => [],
    edit_memory: first(
        obj({ path: str(), text: str() }, OPEN),
        ({ path, text }) => ({ path, text }),
    ),
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
    const settings = part(
        raw.settings,
        ({ reflect_every, allow_net, endpoints }) => ({
            reflect_every,
            allow_net,
            ...(endpoints === undefined
                ? {}
                : {
                      endpoints: endpoints.map(({ name, base_url }) => ({
                          name,
                          base_url,
                      })),
                  }),
        }),
    );
    return {
        ...(raw.charter === undefined ? {} : { charter: raw.charter }),
        ...(model === undefined ? {} : { model }),
        ...(settings === undefined ? {} : { settings }),
    };
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
    manual,
    agrees_roads,
}: Static<typeof PLACE>): Place {
    return {
        version,
        ...(lead === undefined ? {} : { lead }),
        hires,
        roads,
        tools_off: tools_off.map(({ name }) => ({ name })),
        ...(folder === undefined ? {} : { folder }),
        manual: manual ?? false,
        agrees_roads: agrees_roads ?? false,
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
