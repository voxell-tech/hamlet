/**
 * The words and helpers the messages of every actor share. A field
 * that is not set is absent, never `null`. An integer the agent writes
 * is a `u32`.
 */
import {
    compile,
    int,
    type Schema,
    type Static,
} from "@earendil-works/pi-schema";
import { ActorError } from "../../actor/actor/mod.ts";

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

export const OPEN = { open: true } as const;

export const U32 = int({ minimum: 0, maximum: U32_MAX });

export function reader<S extends Schema, T>(
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
