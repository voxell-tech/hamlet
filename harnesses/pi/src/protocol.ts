// The runtime protocol between the Hamlet server and this runtime: its messages are the Rust types of
// crates/hamlet_runtime_protocol, whose JSON Schema protocol-schema.ts holds.
import type { Static } from "typebox";
import Schema from "typebox/schema";
import { schema } from "./protocol-schema.ts";

type Defs = typeof schema.$defs;

/** The type of the schema's definition `N`. */
type Def<N extends keyof Defs> = Static<{ $defs: Defs; $ref: `#/$defs/${N}` }>;

/** Every message the server sends a runtime. */
export type ServerMessage = Def<"ServerMessage">;

/** Every message a runtime sends the server. */
export type RuntimeMessage = Def<"RuntimeMessage">;

/** The message of `type`, in either direction. */
type Message<T extends string> = Extract<
    ServerMessage | RuntimeMessage,
    { type: T }
>;

export type Block = Def<"Block">;
export type Capability = Def<"Capability">;
export type Credential = Def<"Credential">;
export type Item = Def<"Item">;
export type Mark = Def<"Mark">;
export type ModelRef = Def<"ModelRef">;
export type Partial = Def<"Partial">;
export type Selection = Def<"Selection">;
export type ToolSpec = Def<"ToolSpec">;
export type Usage = Def<"Usage">;

export type CredentialRequest = Message<"credential_request">;
export type ProviderControl = Message<"provider_control">;
export type ProviderRequest = Message<"provider_request">;
export type ProviderResult = Message<"provider_result">;
export type Waiting = Message<"waiting">;

/** The protocol version this runtime speaks, the one every message carries as `v`. */
export const PROTOCOL_VERSION =
    schema.$defs.ServerMessage.oneOf[0].properties.v.const;

/** The kind of an entry written for people alone. */
export const NOTICE = "hamlet.notice";

const fromServer = Schema.Compile({
    $defs: schema.$defs,
    $ref: "#/$defs/ServerMessage",
});
const fromRuntime = Schema.Compile({
    $defs: schema.$defs,
    $ref: "#/$defs/RuntimeMessage",
});

/** A line from the server as a protocol message, or the words of why it is not one. */
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
    const [, [error]] = fromServer.Errors(value);
    return {
        ok: false,
        error: `${error?.instancePath || "the message"} ${error?.message ?? "is not a protocol message"}`,
    };
}

/** Whether `value` is a message a runtime may send; for tests of what this runtime writes. */
export function isRuntimeMessage(value: unknown): value is RuntimeMessage {
    return fromRuntime.Check(value);
}
