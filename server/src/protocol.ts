import { type Static, type TProperties, type TSchema, Type } from "typebox";
import { Compile, type Validator } from "typebox/compile";
import type { TLocalizedValidationError } from "typebox/error";

/** The most file bytes a chief can give at once (H25). */
export const MAX_GIFT_BYTES = 32 * 1024 * 1024;

/** An object schema that refuses any field it does not name. */
function object<P extends TProperties>(properties: P) {
    return Type.Object(properties, { additionalProperties: false });
}

function nullable<T extends TSchema>(type: T) {
    return Type.Union([type, Type.Null()]);
}

/** The id of a client's request: a UUID the client makes. */
export const RequestId = Type.String({ format: "uuid" });

/** A client's request of `type`, carrying its id. */
function request<T extends string, P extends TProperties>(
    type: T,
    properties: P,
) {
    return object({ type: Type.Literal(type), id: RequestId, ...properties });
}

/** An agent by its hamlet and its name, or `mayor` for whoever holds the office. */
export const Address = object({ hamlet: Type.String(), agent: Type.String() });
export type Address = Static<typeof Address>;

/** An agent by its hamlet and number (C9). */
export const AgentRef = object({
    hamlet: Type.String(),
    number: Type.Integer(),
});
export type AgentRef = Static<typeof AgentRef>;

/** A chief's chat with an agent, or with `past`, one of its earlier conversations by number. */
export const ChatRef = object({
    with: Address,
    past: Type.Optional(Type.Integer()),
    from: Type.Optional(AgentRef),
    kind: Type.Optional(Type.Enum(["letters", "questions"])),
});
export type ChatRef = Static<typeof ChatRef>;

/** A bounded, sequential part of an accepted file gift (H25). */
export const GivePart = request("give-part", {
    upload: Type.String(),
    path: Type.String(),
    offset: Type.Integer({ minimum: 0 }),
    data: Type.String(),
});
export type GivePart = Static<typeof GivePart>;

/** A chief's answer to a wait (H11). */
export const Choice = Type.Enum([
    "allow",
    "always",
    "deny",
    "go",
    "withdraw",
    "reply",
    "dismiss",
]);
export type Choice = Static<typeof Choice>;

/** Spend on each meter: tokens, micro-dollars and model time in milliseconds. */
export const Amount = object({
    tokens: Type.Integer(),
    micros: Type.Integer(),
    ms: Type.Integer(),
});
export type Amount = Static<typeof Amount>;

/** The lines an edit takes out of a file and puts in. */
export const Change = object({
    path: Type.String(),
    /** True when the edit makes the file. */
    created: Type.Boolean(),
    /** The first lines of the change, with those that stay around them. */
    lines: Type.Array(
        object({
            mark: Type.Enum([" ", "+", "-"]),
            text: Type.String({ maxLength: 240 }),
        }),
        { maxItems: 200 },
    ),
    /** Lines of the whole change, those that stay among them. */
    total: Type.Integer(),
    added: Type.Integer(),
    removed: Type.Integer(),
    /** The lines added and removed, in words. */
    count: Type.String(),
});
export type Change = Static<typeof Change>;

export const Period = Type.Enum(["day", "month"]);
export type Period = Static<typeof Period>;

/** The rules of a hamlet's votes (H13). */
export const VoteLimits = object({
    passYes: Type.Integer(),
    passOf: Type.Integer(),
    leastVoters: Type.Integer(),
    sealed: Type.Boolean(),
    groundedOnly: Type.Boolean(),
    ballotMinutes: Type.Integer(),
    openMinutes: Type.Integer(),
    retryMinutes: Type.Integer(),
});

/** The realm's limits (H14, H19). */
export const HamletLimits = object({
    most: Type.Integer(),
    roadsBetween: Type.Boolean(),
    removalMayors: Type.Integer(),
});

/** A hamlet's limits as stored, where null leaves a meter uncapped (H22). */
export const Limits = object({
    votes: Type.Optional(VoteLimits),
    hamlets: Type.Optional(HamletLimits),
    budget: object({
        period: Period,
        tokens: nullable(Type.Integer()),
        /** Dollars. */
        cost: nullable(Type.Number()),
        minutes: nullable(Type.Integer()),
    }),
    /** The caps on one task (H12). */
    task: object({
        messages: Type.Integer(),
        tokens: nullable(Type.Integer()),
        cost: nullable(Type.Number()),
        minutes: nullable(Type.Integer()),
    }),
    /** The price of one road. */
    road: object({
        tokens: Type.Integer(),
        cost: Type.Number(),
        minutes: Type.Integer(),
        growsOverDays: Type.Integer(),
    }),
    mostAgents: Type.Integer(),
    mostRoads: Type.Integer(),
    /** Models the mayor may choose, besides the hamlet's own. */
    models: Type.Array(Type.String()),
    /** The hamlet's own model, or null for the home's default. */
    model: nullable(Type.String()),
    /** The chief's choice of model for an agent, by name. */
    pinned: Type.Record(Type.String(), Type.String()),
    /** The acts the mayor may do without asking the chief. */
    mayor: object({
        createAgents: Type.Boolean(),
        buildRoads: Type.Boolean(),
        removeAgents: Type.Boolean(),
        mergeAgents: Type.Boolean(),
        toldDays: Type.Integer(),
    }),
    leaving: object({
        protectedMinutes: Type.Integer(),
        mergedNotes: Type.Integer(),
    }),
    asking: object({
        mayorReview: Type.Enum(["always", "never", "remote"]),
        refusals: Type.Integer(),
        questions: Type.Integer(),
    }),
    /** The agents that may reach past the sandbox without asking, by name. */
    permissions: object({
        network: Type.Array(Type.String()),
        unconfined: Type.Array(Type.String()),
    }),
});
export type Limits = Static<typeof Limits>;

export const Permission = Type.Enum(["network", "unconfined"]);
export type Permission = Static<typeof Permission>;

/** Leave an agent has to reach past the sandbox without asking. */
export const Grant = object({
    agent: nullable(AgentRef),
    name: Type.String(),
    permission: Permission,
    /** The chief who gave it in an answer. */
    by: nullable(Type.String()),
    source: Type.Enum(["limits", "grant"]),
});
export type Grant = Static<typeof Grant>;

/** The last check of the home's default model. */
export const Link = object({
    state: Type.Enum(["untested", "testing", "connected", "failed"]),
    models: nullable(Type.Integer()),
    words: nullable(Type.String()),
});
export type Link = Static<typeof Link>;

/** A model's price in dollars per million tokens. */
export const Price = object({ input: Type.Number(), output: Type.Number() });
export type Price = Static<typeof Price>;

export const Endpoint = object({ name: Type.String(), baseUrl: Type.String() });
export type Endpoint = Static<typeof Endpoint>;

export const LoginMethod = Type.Enum(["key", "subscription"]);
export type LoginMethod = Static<typeof LoginMethod>;

/** The state a client saw last. */
export const Seen = object({ epoch: Type.Integer(), revision: Type.Integer() });
export type Seen = Static<typeof Seen>;

/** The first message of every connection, with the home's token (S4). */
export const Hello = object({
    type: Type.Literal("hello"),
    v: Type.Literal(0),
    token: Type.String(),
    seen: Type.Optional(Seen),
});
export type Hello = Static<typeof Hello>;

/** A typed line: words or a command (H24). */
export const Say = request("say", { to: Address, text: Type.String() });
export type Say = Static<typeof Say>;

/** A chief's file or skill gift to an agent (H25). */
export const Give = Type.Union([
    request("give", {
        to: Address,
        files: Type.Array(
            object({
                path: Type.String(),
                size: Type.Integer({
                    minimum: 0,
                    maximum: Number.MAX_SAFE_INTEGER,
                }),
            }),
        ),
    }),
    request("give", {
        to: Address,
        skill: Type.Union([
            object({ starter: Type.String() }),
            object({ from: Address, name: Type.String() }),
        ]),
    }),
]);
export type Give = Static<typeof Give>;

/** A chief's answer to what waits (H11). */
export const Answer = request("answer", {
    wait: Type.String(),
    choice: Choice,
    text: Type.Optional(Type.String()),
});
export type Answer = Static<typeof Answer>;

/** A stop of an agent's work, or with no agent, of everyone's (H23, C25). */
export const Stop = request("stop", { agent: Type.Optional(Address) });
export type Stop = Static<typeof Stop>;

/** A rewind of a chat to before one of its items (C27). */
export const Rewind = request("rewind", { chat: ChatRef, item: Type.String() });
export type Rewind = Static<typeof Rewind>;

/** A new conversation with an agent (C24). */
export const New = request("new", { to: Address });
export type New = Static<typeof New>;

/** A return to an earlier conversation with an agent, by its number (C24). */
export const Resume = request("resume", {
    to: Address,
    number: Type.Integer(),
});
export type Resume = Static<typeof Resume>;

/** A request that an agent carry on from where it stopped (C26). */
export const Retry = request("retry", { to: Address });
export type Retry = Static<typeof Retry>;

/** A chief's edit of one of its documents. */
export const Edit = Type.Union([
    request("edit", {
        doc: Type.Literal("charter"),
        /** Null for the default charter. */
        text: nullable(Type.String()),
    }),
    request("edit", {
        doc: Type.Enum(["character", "notes"]),
        agent: Address,
        text: Type.String(),
    }),
    request("edit", {
        doc: Type.Literal("limits"),
        hamlet: Type.String(),
        reason: Type.Optional(Type.String()),
        set: object({
            section: Type.String(),
            key: Type.String(),
            /** Null takes the key out, so that its default holds. */
            value: Type.Unknown(),
        }),
    }),
    request("edit", {
        doc: Type.Literal("limits"),
        hamlet: Type.String(),
        reason: Type.Optional(Type.String()),
        /** The limits as the chief wrote them, where a field left out takes its default (H22). */
        whole: Type.Record(Type.String(), Type.Unknown()),
    }),
    request("edit", {
        doc: Type.Literal("settings"),
        set: object({
            model: Type.Optional(Type.String()),
            reflectEvery: Type.Optional(Type.Integer()),
            allowNet: Type.Optional(Type.Boolean()),
            /** An endpoint left out is removed, with its credential. */
            endpoints: Type.Optional(Type.Array(Endpoint)),
        }),
    }),
    request("edit", {
        doc: Type.Literal("catalogue"),
        model: Type.String(),
        set: object({
            allowed: Type.Optional(Type.Boolean()),
            price: Type.Optional(nullable(Price)),
            note: Type.Optional(Type.String()),
        }),
    }),
]);
export type Edit = Static<typeof Edit>;

/** A watch of a chat, which begins with the page of its transcript before the item `before`. */
export const Open = request("open", {
    chat: ChatRef,
    before: Type.Optional(Type.String()),
});
export type Open = Static<typeof Open>;

/** The end of a watch of a chat. */
export const Close = object({ type: Type.Literal("close"), chat: ChatRef });
export type Close = Static<typeof Close>;

/** A login started, one of its steps answered, or a login cancelled (S8). */
export const Login = Type.Union([
    request("login", {
        start: object({ provider: Type.String(), method: LoginMethod }),
    }),
    request("login", {
        reply: object({ login: Type.String(), value: Type.String() }),
    }),
    request("login", { cancel: Type.String() }),
]);
export type Login = Static<typeof Login>;

/** A try of a model, or of the default model's endpoint. */
export const Check = Type.Union([
    request("check", {
        what: Type.Literal("model"),
        model: Type.Optional(Type.String()),
    }),
    request("check", { what: Type.Literal("connection") }),
]);
export type Check = Static<typeof Check>;

/** A request for something the state does not carry. */
export const Read = Type.Union([
    request("read", { what: Type.Literal("prompt"), agent: Address }),
    request("read", { what: Type.Literal("history"), agent: Address }),
    request("read", {
        what: Type.Literal("models"),
        refresh: Type.Optional(Type.Boolean()),
    }),
    request("read", {
        what: Type.Literal("papers"),
        agent: Address,
        /** Uncut text for the chief's editor. */
        full: Type.Optional(Type.Boolean()),
    }),
    request("read", { what: Type.Literal("charter") }),
]);
export type Read = Static<typeof Read>;

const FROM_CLIENT = {
    hello: Hello,
    say: Say,
    give: Give,
    "give-part": GivePart,
    answer: Answer,
    stop: Stop,
    rewind: Rewind,
    new: New,
    resume: Resume,
    retry: Retry,
    edit: Edit,
    open: Open,
    close: Close,
    login: Login,
    check: Check,
    read: Read,
};

/** A message a client sends. */
export const ClientMessage = Type.Union(Object.values(FROM_CLIENT));
export type ClientMessage = Static<
    (typeof FROM_CLIENT)[keyof typeof FROM_CLIENT]
>;

/** A hamlet's spend in its period, beside its caps (H8). */
export const Report = object({
    period: Period,
    periodWords: Type.String(),
    /** The share of the period gone, from 0 to 1. */
    gone: Type.Number(),
    spent: Amount,
    caps: object({
        micros: nullable(Type.Integer()),
        ms: nullable(Type.Integer()),
    }),
    summary: Type.String(),
    lines: Type.Array(Type.String()),
    /** The share left on the meter nearest its cap, from 0 to 1. */
    left: nullable(Type.Number()),
    /** The rest on the meter nearest its cap, in words. */
    remains: nullable(Type.String()),
});
export type Report = Static<typeof Report>;

/** A road with its ends resolved to living agents (H3). */
export const Road = object({
    a: AgentRef,
    b: AgentRef,
    messages: Type.Integer(),
    level: Type.Enum(["overgrown", "dirt", "paved", "rail"]),
    lastFrom: nullable(AgentRef),
});
export type Road = Static<typeof Road>;

/** A voter's ballot, or null before it is asked. */
export const Voter = object({
    agent: AgentRef,
    cast: nullable(Type.Enum(["yes", "no", "abstain", "none"])),
});

/** A vote's ready-made words (H13). */
export const VoteView = object({
    id: Type.Integer(),
    motion: Type.String(),
    by: Type.String(),
    open: Type.Boolean(),
    overturnable: Type.Boolean(),
    report: Type.String(),
    voters: Type.Array(Voter),
});
export type VoteView = Static<typeof VoteView>;

/** A town's position (H32). */
export const TownAt = object({ x: Type.Number(), z: Type.Number() });

/** An agent's departure and farewell (H28). */
export const Left = object({
    how: Type.Enum(["removed", "merged"]),
    into: nullable(AgentRef),
    at: Type.Integer(),
    by: Type.String(),
    reason: Type.String(),
    words: Type.String(),
});

/** A conversation with another agent (C22). */
export const ConversationView = object({
    with: AgentRef,
    kind: Type.Enum(["letters", "questions"]),
    count: Type.Integer(),
    last: nullable(Type.Integer()),
});

/** A skill on the library's shelf (H33). */
export const LibrarySkill = object({
    name: Type.String(),
    description: Type.String(),
    by: Type.String(),
});

/** A removed hamlet a chief can restore or erase (H20). */
export const ClosedHamlet = object({
    id: Type.String(),
    name: Type.String(),
    at: Type.Integer(),
    by: Type.String(),
    reason: Type.String(),
});

export const Hamlet = object({
    roads: Type.Optional(Type.Array(Road)),
    votes: Type.Optional(Type.Array(VoteView)),
    at: Type.Optional(nullable(TownAt)),
    id: Type.String(),
    name: Type.String(),
    folder: nullable(Type.String()),
    /** Folders it worked on before. */
    before: Type.Array(Type.String()),
    mode: Type.Enum(["manual", "auto"]),
    modeWords: Type.String(),
    limits: Limits,
    budget: Report,
});
export type Hamlet = Static<typeof Hamlet>;

/** The step of its work an agent is in. */
export const Doing = Type.Enum([
    "rest",
    "think",
    "read-file",
    "read-notes",
    "read-skill",
    "write-file",
    "write-notes",
    "write-skill",
    "run",
    "fetch",
    "custom",
    "await",
]);
export type Doing = Static<typeof Doing>;

export const Agent = object({
    left: Type.Optional(Left),
    consulting: Type.Optional(Type.Boolean()),
    conversations: Type.Optional(Type.Array(ConversationView)),
    ref: AgentRef,
    name: Type.String(),
    purpose: Type.String(),
    /** Its arrival in the hamlet, in milliseconds since the epoch. */
    since: Type.Integer(),
    mayor: Type.Boolean(),
    model: Type.String(),
    /** A conversation of the agent runs or holds a queued item. */
    busy: Type.Boolean(),
    /** Its chief chat's last turn failed, and nothing runs now. */
    faulted: Type.Boolean(),
    activity: object({
        doing: Doing,
        doingWords: Type.String(),
        /** Model calls and tool calls begun since it took up its work. */
        step: Type.Integer(),
        /** Tool calls since then that ended in an error. */
        failures: Type.Integer(),
    }),
    saying: object({ text: Type.String(), thought: Type.Boolean() }),
    spent: object({ ...Amount.properties, cost: Type.String() }),
    /** Skill folders in its house (H25). */
    skills: Type.Optional(Type.Array(Type.String())),
});
export type Agent = Static<typeof Agent>;

/** Something that waits for a chief (H11). */
const waitFields = {
    id: Type.String(),
    by: Type.String(),
    agent: nullable(AgentRef),
    says: Type.String(),
    about: Type.String(),
    change: nullable(Change),
    answers: Type.Array(Choice),
    since: Type.Integer(),
    asked: Type.Integer(),
    chat: nullable(ChatRef),
};
export const WaitDanger = Type.Enum([
    "network",
    "unconfined",
    "destructive",
    "reserved",
    "edit",
    "command",
]);
export const Wait = Type.Union([
    object({
        ...waitFields,
        kind: Type.Literal("approval"),
        danger: WaitDanger,
    }),
    object({ ...waitFields, kind: Type.Literal("fault"), danger: Type.Null() }),
    object({
        ...waitFields,
        kind: Type.Literal("question"),
        danger: Type.Null(),
    }),
    object({
        ...waitFields,
        kind: Type.Literal("objection"),
        danger: Type.Null(),
    }),
]);
export type Wait = Static<typeof Wait>;

/** The home's settings, with no credential in them. */
export const SettingsView = object({
    model: Type.String(),
    reflectEvery: Type.Integer(),
    allowNet: Type.Boolean(),
    endpoints: Type.Array(
        object({
            ...Endpoint.properties,
            /** A key is kept for it. */
            key: Type.Boolean(),
        }),
    ),
    /** Nothing is set up yet. */
    unset: Type.Boolean(),
    link: Link,
});
export type SettingsView = Static<typeof SettingsView>;

/** A model provider a chief can sign in to or give a key for. */
export const ProviderView = object({
    id: Type.String(),
    name: Type.String(),
    configured: Type.Boolean(),
    methods: Type.Array(LoginMethod),
    label: Type.String(),
});
export type ProviderView = Static<typeof ProviderView>;

/** The whole realm's state (S5). */
export const State = object({
    library: Type.Optional(Type.Array(LibrarySkill)),
    closed: Type.Optional(Type.Array(ClosedHamlet)),
    type: Type.Literal("state"),
    /** A new epoch is a new server start. */
    epoch: Type.Integer(),
    revision: Type.Integer(),
    chief: Type.String(),
    sandbox: Type.String(),
    hamlets: Type.Array(Hamlet),
    agents: Type.Array(Agent),
    waits: Type.Array(Wait),
    grants: Type.Array(Grant),
    settings: SettingsView,
    providers: Type.Array(ProviderView),
    /** The chief's command names this server carries out (H26). */
    deeds: Type.Optional(Type.Array(Type.String())),
    /** Shipped skills a chief can give (H25). */
    starters: Type.Optional(
        Type.Array(object({ name: Type.String(), description: Type.String() })),
    ),
});
export type State = Static<typeof State>;

/** One whole entity of the state, replaced or taken out. */
export const Update = Type.Union([
    object({ type: Type.Literal("hamlet"), hamlet: Hamlet }),
    object({ type: Type.Literal("agent"), agent: Agent }),
    object({ type: Type.Literal("agent-gone"), ref: AgentRef }),
    object({ type: Type.Literal("wait"), wait: Wait }),
    object({
        type: Type.Literal("wait-closed"),
        id: Type.String(),
        how: Type.String(),
    }),
    object({ type: Type.Literal("grants"), grants: Type.Array(Grant) }),
    object({ type: Type.Literal("settings"), settings: SettingsView }),
    object({
        type: Type.Literal("providers"),
        providers: Type.Array(ProviderView),
    }),
]);
export type Update = Static<typeof Update>;

/** Changes to the state, at a revision that only grows within an epoch. */
export const Changes = object({
    type: Type.Literal("change"),
    epoch: Type.Integer(),
    revision: Type.Integer(),
    changes: Type.Array(Update),
});
export type Changes = Static<typeof Changes>;

/** Model calls of a conversation and what they spent. */
export const UsageView = object({
    calls: Type.Integer(),
    /** Calls whose provider reported no tokens. */
    unreported: Type.Integer(),
    input: Type.Integer(),
    cached: Type.Integer(),
    output: Type.Integer(),
    reasoning: Type.Integer(),
    ms: Type.Integer(),
    micros: Type.Integer(),
    lastInput: Type.Integer(),
    summary: Type.String(),
});
export type UsageView = Static<typeof UsageView>;

/** An item of a transcript (C30). */
export const Item = object({
    /** `<entryId>:<block>`. */
    id: Type.String(),
    /** Milliseconds since the epoch. */
    at: Type.Integer(),
    kind: Type.Enum([
        "user",
        "assistant",
        "reasoning",
        "tool",
        "notice",
        "error",
    ]),
    text: Type.String(),
    chief: Type.Optional(Type.String()),
    /** The sending agent as name@hamlet, or hamlet#number if its record is gone. Never a chief. */
    author: Type.Optional(Type.String()),
    rewindable: Type.Optional(Type.Boolean()),
    tool: Type.Optional(
        object({
            name: Type.String(),
            summary: Type.String(),
            /** Null while it runs. */
            exit: nullable(Type.Integer()),
            ms: nullable(Type.Integer()),
            failure: nullable(Type.String()),
            change: nullable(Change),
        }),
    ),
});
export type Item = Static<typeof Item>;

/** A conversation's latest tool call, reply or error, in words for a chief. */
export const LastActivity = object({ what: Type.String(), at: Type.Integer() });
export type LastActivity = Static<typeof LastActivity>;

/** One step of an open chat's transcript, as it happens. */
export const Op = Type.Union([
    object({
        op: Type.Literal("add"),
        item: Item,
        /** The live item this one takes the place of: that one leaves, and this one goes last. */
        replaces: Type.Optional(Type.String()),
    }),
    object({
        op: Type.Literal("append"),
        item: Type.String(),
        text: Type.String(),
    }),
    object({
        op: Type.Literal("close"),
        item: Type.String(),
        exit: Type.Integer(),
        ms: nullable(Type.Integer()),
        failure: nullable(Type.String()),
        /** The completed write's change, computed before it ran. */
        change: Type.Optional(nullable(Change)),
    }),
    object({
        op: Type.Literal("status"),
        working: Type.Boolean(),
        pending: Type.Integer(),
        last: nullable(LastActivity),
    }),
    object({ op: Type.Literal("usage"), usage: UsageView }),
    /** The transcript is sent again next. */
    object({ op: Type.Literal("reset") }),
]);
export type Op = Static<typeof Op>;

/** The latest steps of an open chat (C30). */
export const Turn = object({
    type: Type.Literal("turn"),
    chat: ChatRef,
    /** The conversation's stable number in the chief's chat. */
    number: Type.Optional(Type.Integer()),
    ops: Type.Array(Op),
});
export type Turn = Static<typeof Turn>;

/** A page of a chat's transcript, as it stands. */
export const Transcript = object({
    type: Type.Literal("transcript"),
    to: Type.Optional(RequestId),
    chat: ChatRef,
    /** The conversation's stable number in the chief's chat. */
    number: Type.Optional(Type.Integer()),
    items: Type.Array(Item),
    /** The item the page before this one ends before, or null at the start. */
    earlier: nullable(Type.String()),
    last: Type.Optional(nullable(LastActivity)),
    working: Type.Boolean(),
    pending: Type.Integer(),
    usage: UsageView,
});
export type Transcript = Static<typeof Transcript>;

/** An answer in words to a request. */
export const Reply = object({
    type: Type.Literal("reply"),
    to: RequestId,
    /** `rewound` gives back the words of the message taken back. */
    kind: Type.Enum(["notice", "error", "rewound"]),
    text: Type.String(),
    /** Accepted file-gift upload, awaiting give-part messages. */
    upload: Type.Optional(Type.String()),
});
export type Reply = Static<typeof Reply>;

export const LoginStep = Type.Union([
    object({ url: Type.String(), instructions: Type.Optional(Type.String()) }),
    object({ code: Type.String() }),
    object({ prompt: Type.String(), secret: Type.Boolean() }),
    object({ progress: Type.String() }),
    object({ done: Type.String() }),
    object({ failed: Type.String() }),
]);
export type LoginStep = Static<typeof LoginStep>;

/** A login's next step (S8). */
export const LoginProgress = object({
    type: Type.Literal("login"),
    login: Type.String(),
    to: Type.Optional(RequestId),
    step: LoginStep,
});
export type LoginProgress = Static<typeof LoginProgress>;

/** A past or current conversation with an agent (C24). */
export const Past = object({
    number: Type.Integer(),
    current: Type.Boolean(),
    title: Type.String(),
    by: Type.String(),
    began: Type.Integer(),
    modified: Type.Integer(),
    messages: Type.Integer(),
    tokens: Type.Integer(),
});
export type Past = Static<typeof Past>;

/** A model of the home's catalogue. */
export const Listed = object({
    name: Type.String(),
    price: nullable(Price),
    priced: Type.Enum(["unknown", "listed", "assigned", "yours"]),
    context: nullable(Type.Integer()),
    allowed: Type.Boolean(),
    note: Type.String(),
});
export type Listed = Static<typeof Listed>;

/** An agent's notes, character and skills. Text is cut to 6000 characters unless the read asks for `full`. */
export const Papers = object({
    notes: Type.String(),
    character: Type.String(),
    skills: Type.Array(
        object({ name: Type.String(), description: Type.String() }),
    ),
});
export type Papers = Static<typeof Papers>;

/** The answer to a `read`. */
export const ReadResult = Type.Union([
    object({
        type: Type.Literal("read"),
        to: RequestId,
        prompt: Type.String(),
    }),
    object({
        type: Type.Literal("read"),
        to: RequestId,
        history: Type.Array(Past),
    }),
    object({
        type: Type.Literal("read"),
        to: RequestId,
        models: Type.Array(Listed),
    }),
    object({ type: Type.Literal("read"), to: RequestId, papers: Papers }),
    object({
        type: Type.Literal("read"),
        to: RequestId,
        /** `own` is true when the home keeps a charter of its own. */
        charter: object({ text: Type.String(), own: Type.Boolean() }),
    }),
]);
export type ReadResult = Static<typeof ReadResult>;

/** A failure in words for a chief. */
export const Fault = object({
    type: Type.Literal("fault"),
    text: Type.String(),
    to: Type.Optional(RequestId),
});
export type Fault = Static<typeof Fault>;

const FROM_SERVER = {
    state: State,
    change: Changes,
    turn: Turn,
    transcript: Transcript,
    reply: Reply,
    login: LoginProgress,
    read: ReadResult,
    fault: Fault,
};

/** A message the server sends. */
export const ServerMessage = Type.Union(Object.values(FROM_SERVER));
export type ServerMessage = Static<
    (typeof FROM_SERVER)[keyof typeof FROM_SERVER]
>;

/** Every named shape of protocol v0. */
const DEFINITIONS: Record<string, TSchema> = {
    RequestId,
    Address,
    AgentRef,
    ChatRef,
    VoteLimits,
    HamletLimits,
    Road,
    Voter,
    VoteView,
    TownAt,
    Left,
    ConversationView,
    LibrarySkill,
    ClosedHamlet,
    Choice,
    Amount,
    Change,
    Period,
    Limits,
    Permission,
    Grant,
    Link,
    Price,
    Endpoint,
    LoginMethod,
    Seen,
    Hello,
    Say,
    Give,
    GivePart,
    Answer,
    Stop,
    Rewind,
    New,
    Resume,
    Retry,
    Edit,
    Open,
    Close,
    Login,
    Check,
    Read,
    ClientMessage,
    Report,
    Hamlet,
    Doing,
    Agent,
    Wait,
    WaitDanger,
    SettingsView,
    ProviderView,
    State,
    Update,
    Changes,
    UsageView,
    Item,
    Op,
    Turn,
    Transcript,
    Reply,
    LoginStep,
    LoginProgress,
    Past,
    Listed,
    Papers,
    ReadResult,
    Fault,
    ServerMessage,
};

/** Protocol v0 as one JSON Schema document, where each named shape is defined once and referred to. */
export function protocolSchema(): Record<string, unknown> {
    const names = new Map(
        Object.entries(DEFINITIONS).map(([name, schema]) => [
            JSON.stringify(schema),
            name,
        ]),
    );
    const refer = (node: unknown, own = false): unknown => {
        if (typeof node !== "object" || node === null) return node;
        const name = own ? undefined : names.get(JSON.stringify(node));
        if (name !== undefined) return { $ref: `#/$defs/${name}` };
        if (Array.isArray(node)) return node.map((item) => refer(item));
        return Object.fromEntries(
            Object.entries(node).map(([key, value]) => [key, refer(value)]),
        );
    };
    return {
        $schema: "https://json-schema.org/draft/2020-12/schema",
        title: "Hamlet protocol v0",
        anyOf: [
            { $ref: "#/$defs/ClientMessage" },
            { $ref: "#/$defs/ServerMessage" },
        ],
        $defs: Object.fromEntries(
            Object.entries(DEFINITIONS).map(([name, schema]) => [
                name,
                refer(schema, true),
            ]),
        ),
    };
}

/** A message read from its JSON text, or the words for why it does not fit protocol v0. */
export type Parsed<M> =
    | { readonly message: M }
    | {
          readonly malformed: string;
          /** The id of the request it was meant to be, when that id is a UUID. */
          readonly to?: string;
      };

/** One form of a message, with its own validator. */
type Form = { readonly properties: TProperties; readonly validator: Validator };

/** The forms of each message, by its type. */
type Forms = ReadonlyMap<string, readonly Form[]>;

function byType(messages: Record<string, TSchema>): Forms {
    return new Map(
        Object.entries(messages).map(([type, schema]) => [
            type,
            (Type.IsUnion(schema) ? schema.anyOf : [schema]).map((form) => ({
                properties: Type.IsObject(form) ? form.properties : {},
                validator: Compile(form),
            })),
        ]),
    );
}

const clientForms = byType(FROM_CLIENT);
const serverForms = byType(FROM_SERVER);
const requestIds = Compile(RequestId);

/** Reads a message a client sent. */
export function parseClient(text: string): Parsed<ClientMessage> {
    return parse(text, clientForms);
}

/** Reads a message the server sent. */
export function parseServer(text: string): Parsed<ServerMessage> {
    return parse(text, serverForms);
}

function parse<M>(text: string, messages: Forms): Parsed<M> {
    let value: unknown;
    try {
        value = JSON.parse(text);
    } catch {
        value = undefined;
    }
    if (!isRecord(value) || typeof value.type !== "string") {
        return { malformed: "A message is a JSON object with a `type`." };
    }
    const { type, id } = value;
    const to = requestIds.Check(id) ? { to: id } : {};
    const forms = messages.get(type);
    if (forms === undefined) {
        return { malformed: `There is no \`${type}\` message.`, ...to };
    }
    if (forms.some((form) => form.validator.Check(value))) {
        return { message: value as M };
    }
    return {
        malformed: `This \`${type}\` message does not fit protocol v0: ${words(value, forms)}.`,
        ...to,
    };
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** The reason `value` fits none of `forms`, in words, said of the form it was meant to take. */
function words(value: Record<string, unknown>, forms: readonly Form[]): string {
    const form = meant(value, forms);
    const [stray] = strays(value, form);
    if (stray !== undefined) {
        const values = new Set(
            forms.flatMap((other) => fixed(other.properties[stray]) ?? []),
        );
        const among = values.size === 1 ? "" : "one of ";
        return `\`${stray}\` must be ${among}${quoted([...values])}`;
    }
    const error = form.validator
        .Errors(value)
        .find(
            (error) => error.keyword !== "anyOf" && error.keyword !== "boolean",
        );
    return error === undefined ? "it is not valid" : said(error);
}

/** The form `value` was meant to take: the one whose fixed fields it matches most, then the one that names most of its fields. */
function meant(value: Record<string, unknown>, forms: readonly Form[]): Form {
    const misses = (form: Form): [number, number] => [
        strays(value, form).length,
        Object.keys(value).filter((key) => !Object.hasOwn(form.properties, key))
            .length,
    ];
    return forms
        .map((form) => ({ form, misses: misses(form) }))
        .reduce((best, next) => {
            const [unmatched, unnamed] = next.misses;
            const fewer =
                unmatched - best.misses[0] || unnamed - best.misses[1];
            return fewer < 0 ? next : best;
        }).form;
}

/** The fields `form` fixes to values that `value` does not hold. */
function strays(value: Record<string, unknown>, form: Form): string[] {
    return Object.entries(form.properties)
        .filter(([key, schema]) => {
            const values = fixed(schema);
            return values !== undefined && !values.includes(value[key]);
        })
        .map(([key]) => key);
}

/** The values a field must take, when its schema fixes them. */
function fixed(schema: unknown): readonly unknown[] | undefined {
    if (Type.IsLiteral(schema)) return [schema.const];
    if (Type.IsEnum(schema)) return schema.enum;
    return undefined;
}

function said(error: TLocalizedValidationError): string {
    const at =
        error.instancePath === ""
            ? "it"
            : `\`${error.instancePath.slice(1).replaceAll("/", ".")}\``;
    switch (error.keyword) {
        case "required":
            return `${at} lacks ${quoted(error.params.requiredProperties)}`;
        case "additionalProperties":
            return `${at} has no field ${quoted(error.params.additionalProperties)}`;
        case "format":
            if (error.params.format === "uuid") return `${at} must be a UUID`;
    }
    return `${at} ${error.message}`;
}

function quoted(values: readonly unknown[]): string {
    return values.map((value) => `\`${String(value)}\``).join(", ");
}
