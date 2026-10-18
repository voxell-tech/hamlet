import { type Static, type TProperties, type TSchema, Type } from "typebox";
import { Compile } from "typebox/compile";

/** The link version this runtime speaks. */
export const LINK_VERSION = 1;

/** An object schema that refuses any field it does not name. */
function object<P extends TProperties>(properties: P) {
    return Type.Object(properties, { additionalProperties: false });
}

/** A message of `type`, with the link version and the sender's own id for it. */
function message<T extends string, P extends TProperties>(
    type: T,
    properties: P,
) {
    return object({
        type: Type.Literal(type),
        v: Type.Integer({ minimum: LINK_VERSION, maximum: LINK_VERSION }),
        id: Type.String({ minLength: 1 }),
        ...properties,
    });
}

/** A reply of `type` to the message whose id is `re`. */
function reply<T extends string, P extends TProperties>(
    type: T,
    properties: P,
) {
    return message(type, { re: Type.String({ minLength: 1 }), ...properties });
}

/** A conversation by the id its runtime made for it. */
const ConversationId = Type.String({ minLength: 1 });

/** What a runtime can do; the server refuses one that lacks what Hamlet needs. */
export const Capability = Type.Enum([
    "operations",
    "attempts",
    "admission",
    "cancel",
    "recovery",
    "updates",
    "history",
]);
export type Capability = Static<typeof Capability>;

/** A model by its provider and id, and the address it is served at when that is not the provider's own. */
export const ModelRef = object({
    provider: Type.String({ minLength: 1 }),
    id: Type.String({ minLength: 1 }),
    baseUrl: Type.Optional(Type.String({ minLength: 1 })),
});
export type ModelRef = Static<typeof ModelRef>;

/** A secret the server hands over for one model call, held in memory alone. */
export const Credential = object({
    type: Type.Literal("api_key"),
    key: Type.String(),
});
export type Credential = Static<typeof Credential>;

/** Tokens of one model call and its cost in dollars. */
export const Usage = object({
    input: Type.Integer({ minimum: 0 }),
    output: Type.Integer({ minimum: 0 }),
    cacheRead: Type.Integer({ minimum: 0 }),
    cacheWrite: Type.Integer({ minimum: 0 }),
    cost: Type.Number({ minimum: 0 }),
});
export type Usage = Static<typeof Usage>;

/** A tool the server runs, as the model is offered it. */
export const ToolSpec = object({
    name: Type.String({ minLength: 1 }),
    description: Type.String(),
    /** The JSON Schema of its arguments. */
    parameters: Type.Record(Type.String(), Type.Unknown()),
    /** Whether a call cut off before its result was kept may run again; one that may not answers that it was cut off. */
    replay: Type.Enum(["safe", "unsafe"]),
});
export type ToolSpec = Static<typeof ToolSpec>;

/** A tool call in an assistant's answer. */
export const Call = object({
    id: Type.String(),
    name: Type.String(),
    arguments: Type.Record(Type.String(), Type.Unknown()),
});
export type Call = Static<typeof Call>;

/** One kept entry of a conversation, as the server shows it. */
export const Item = object({
    entry: Type.String({ minLength: 1 }),
    kind: Type.Enum(["user", "assistant", "tool_result", "other"]),
    /** The request id of the input that placed a user entry. */
    request: Type.Optional(Type.String()),
    text: Type.String(),
    thinking: Type.Optional(Type.String()),
    calls: Type.Optional(Type.Array(Call)),
    /** The call a tool result answers. */
    call: Type.Optional(Type.String()),
    error: Type.Optional(Type.Boolean()),
    /** Why an assistant's answer ended. */
    stop: Type.Optional(Type.String()),
    usage: Type.Optional(Usage),
});
export type Item = Static<typeof Item>;

/** A prompt section by name, rendered in order before each model call. */
export const Section = object({
    name: Type.String({ minLength: 1 }),
    text: Type.String(),
});
export type Section = Static<typeof Section>;

/** Why a message could not be handled. */
export const ErrorCode = Type.Enum([
    "malformed",
    "unknown_conversation",
    "unsupported",
    "failed",
]);
export type ErrorCode = Static<typeof ErrorCode>;

/** A message that could not be handled, in either direction; `re` names it when its id was readable. */
export const LinkError = message("error", {
    re: Type.Optional(Type.String()),
    code: ErrorCode,
    message: Type.String(),
});
export type LinkError = Static<typeof LinkError>;

/** The runtime's first message: what it speaks, what it is and what its store records. */
export const Hello = message("hello", {
    versions: Type.Array(Type.Integer({ minimum: 1 })),
    adapter: Type.String(),
    harness: Type.String(),
    /** The runtime id its store records; null when there is no store. */
    runtime: Type.Union([Type.String(), Type.Null()]),
    store: Type.Enum(["created", "opened", "missing"]),
    capabilities: Type.Array(Capability),
});
export type Hello = Static<typeof Hello>;

/** The server takes the runtime on, with the tools it runs for the runtime's conversations. */
export const Welcome = reply("welcome", {
    version: Type.Integer({ minimum: 1 }),
    tools: Type.Array(ToolSpec),
});
export type Welcome = Static<typeof Welcome>;

/** The server will not take the runtime on, in words for a chief; the runtime exits. */
export const Refused = reply("refused", { words: Type.String() });
export type Refused = Static<typeof Refused>;

/** Creates a conversation once per `key`; asking again answers with the same conversation. */
export const Create = message("create", {
    key: Type.String({ minLength: 1 }),
    /** The agent it is for, as `<hamlet>:<number>`. */
    agent: Type.String({ pattern: "^[^:]+:[0-9]+$" }),
    errand: Type.Boolean(),
    model: ModelRef,
});
export type Create = Static<typeof Create>;

export const Created = reply("created", { conversation: ConversationId });
export type Created = Static<typeof Created>;

/** An input for a conversation, admitted once by its request id. */
export const Submit = message("submit", {
    conversation: ConversationId,
    request: Type.String({ minLength: 1 }),
    content: Type.String(),
    steer: Type.Boolean(),
});
export type Submit = Static<typeof Submit>;

/** The input is kept in the runtime's store; a repeated request answers as the first did. */
export const Admitted = reply("admitted", {
    submission: Type.String(),
    status: Type.Enum(["queued", "placed", "done", "unanswered"]),
});
export type Admitted = Static<typeof Admitted>;

/** Stops a conversation's running turn and drops its queue. */
export const Cancel = message("cancel", { conversation: ConversationId });
export type Cancel = Static<typeof Cancel>;

export const Cancelled = reply("cancelled", {});
export type Cancelled = Static<typeof Cancelled>;

/** A page of a conversation's kept entries, newest last, before the entry `before`. */
export const History = message("history", {
    conversation: ConversationId,
    before: Type.Optional(Type.String()),
    limit: Type.Integer({ minimum: 1, maximum: 200 }),
});
export type History = Static<typeof History>;

export const Page = reply("page", {
    items: Type.Array(Item),
    earlier: Type.Boolean(),
});
export type Page = Static<typeof Page>;

/** What each of the runtime's conversations is doing. */
export const Status = message("status", {});
export type Status = Static<typeof Status>;

export const Statuses = reply("statuses", {
    conversations: Type.Array(
        object({ conversation: ConversationId, running: Type.Boolean() }),
    ),
});
export type Statuses = Static<typeof Statuses>;

/** A wait for the operation `key` was answered, so asking again may get further. */
export const Answered = message("answered", {
    key: Type.String({ minLength: 1 }),
});
export type Answered = Static<typeof Answered>;

/** Asks early whether a tool call may run, so a wait shows before the call is kept as begun. */
export const Allow = message("allow", {
    conversation: ConversationId,
    key: Type.String({ minLength: 1 }),
    tool: Type.String({ minLength: 1 }),
    arguments: Type.Record(Type.String(), Type.Unknown()),
});
export type Allow = Static<typeof Allow>;

/** Carries out a tool call under its operation key; the server decides as it runs. */
export const Operation = message("operation", {
    conversation: ConversationId,
    key: Type.String({ minLength: 1 }),
    tool: Type.String({ minLength: 1 }),
    arguments: Type.Record(Type.String(), Type.Unknown()),
});
export type Operation = Static<typeof Operation>;

/** The call may run. */
export const Allowed = reply("allowed", {});
export type Allowed = Static<typeof Allowed>;

/** The call waits for a chief; nothing is kept of it as a result. */
export const Waiting = reply("waiting", { words: Type.String() });
export type Waiting = Static<typeof Waiting>;

/** The call may not run, in words for the agent. */
export const Refusal = reply("refusal", { words: Type.String() });
export type Refusal = Static<typeof Refusal>;

/** The marked result of an operation, for the agent. */
export const Result = reply("result", {
    text: Type.String(),
    error: Type.Boolean(),
});
export type Result = Static<typeof Result>;

/** Asks before each model call. */
export const Attempt = message("attempt", {
    conversation: ConversationId,
    key: Type.String({ minLength: 1 }),
    model: ModelRef,
});
export type Attempt = Static<typeof Attempt>;

/** The call may be made, with the credential it needs. */
export const Allowance = reply("allowance", {
    credential: Type.Optional(Credential),
});
export type Allowance = Static<typeof Allowance>;

/** The call may not be made, in words of what ran out. */
export const Stopped = reply("stopped", { words: Type.String() });
export type Stopped = Static<typeof Stopped>;

/** The usage of an allowed attempt, before its answer is kept. */
export const UsageReport = message("usage", {
    key: Type.String({ minLength: 1 }),
    usage: Usage,
});
export type UsageReport = Static<typeof UsageReport>;

/** The usage is charged, once however often it was reported. */
export const Charged = reply("charged", {});
export type Charged = Static<typeof Charged>;

/** Asks for a conversation's prompt sections before each model call. */
export const Prompt = message("prompt", { conversation: ConversationId });
export type Prompt = Static<typeof Prompt>;

export const Sections = reply("sections", { sections: Type.Array(Section) });
export type Sections = Static<typeof Sections>;

/** An entry the runtime kept. */
export const Kept = message("kept", {
    conversation: ConversationId,
    item: Item,
});
export type Kept = Static<typeof Kept>;

/** A conversation started or ended a run. */
export const Running = message("running", {
    conversation: ConversationId,
    running: Type.Boolean(),
});
export type Running = Static<typeof Running>;

/** Every message the server sends a runtime. */
export const ServerMessage = Type.Union([
    Welcome,
    Refused,
    Create,
    Submit,
    Cancel,
    History,
    Status,
    Answered,
    Allowed,
    Waiting,
    Refusal,
    Result,
    Allowance,
    Stopped,
    Charged,
    Sections,
    LinkError,
]);
export type ServerMessage = Static<typeof ServerMessage>;

/** Every message a runtime sends the server. */
export const RuntimeMessage = Type.Union([
    Hello,
    Created,
    Admitted,
    Cancelled,
    Page,
    Statuses,
    Allow,
    Operation,
    Attempt,
    UsageReport,
    Prompt,
    Kept,
    Running,
    LinkError,
]);
export type RuntimeMessage = Static<typeof RuntimeMessage>;

/** The named definitions of the link, in the order the schema lists them. */
const DEFINITIONS: Record<string, TSchema> = {
    Capability,
    ModelRef,
    Credential,
    Usage,
    ToolSpec,
    Call,
    Item,
    Section,
    ErrorCode,
    LinkError,
    Hello,
    Welcome,
    Refused,
    Create,
    Created,
    Submit,
    Admitted,
    Cancel,
    Cancelled,
    History,
    Page,
    Status,
    Statuses,
    Answered,
    Allow,
    Operation,
    Allowed,
    Waiting,
    Refusal,
    Result,
    Attempt,
    Allowance,
    Stopped,
    UsageReport,
    Charged,
    Prompt,
    Sections,
    Kept,
    Running,
    ServerMessage,
    RuntimeMessage,
};

/** The link as JSON Schema, with each named definition referred to by `$ref`. */
export function linkSchema(): Record<string, unknown> {
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
        title: "Hamlet harness link v1",
        anyOf: [
            { $ref: "#/$defs/ServerMessage" },
            { $ref: "#/$defs/RuntimeMessage" },
        ],
        $defs: Object.fromEntries(
            Object.entries(DEFINITIONS).map(([name, schema]) => [
                name,
                refer(schema, true),
            ]),
        ),
    };
}

const fromServer = Compile(ServerMessage);
const fromRuntime = Compile(RuntimeMessage);

/** A line from the server as a link message, or the words of why it is not one. */
export function parseServerLine(
    line: string,
): { ok: true; value: ServerMessage } | { ok: false; error: string } {
    let value: unknown;
    try {
        value = JSON.parse(line);
    } catch {
        return { ok: false, error: "not JSON" };
    }
    if (fromServer.Check(value)) return { ok: true, value };
    const [error] = fromServer.Errors(value);
    return {
        ok: false,
        error: `${error?.instancePath || "the message"} ${error?.message ?? "is not a link message"}`,
    };
}

/** Whether `value` is a message a runtime may send; for tests of what this runtime writes. */
export function isRuntimeMessage(value: unknown): value is RuntimeMessage {
    return fromRuntime.Check(value);
}
