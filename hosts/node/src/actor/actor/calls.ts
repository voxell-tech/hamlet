import type { ActorDefinition } from "./definition.ts";
import { ActorError, internalError } from "./errors.ts";

declare const actionsOf: unique symbol;

/**
 * An actor's address, as `hamlet_host::Address`: a kind and one key, both
 * plain names. `A` is the kind's actions as a caller sees them, and exists
 * only in the type. Made by `Address.of`, or handed in by the host.
 */
export interface Address<A = unknown> {
    readonly kind: string;
    readonly key: string;
    readonly [actionsOf]: A;
}

/**
 * Internal: `hamlet_host::address::plain`. Every character it accepts is one
 * byte, so its length in characters is its length in bytes.
 */
export const PLAIN_NAME = /^(?!\.)[a-z0-9._-]{1,128}$/;

export const Address = {
    /** Throws `napi.invalid_argument` for a name that is not plain. */
    of<A = unknown>(kind: string, key: string): Address<A> {
        for (const name of [kind, key]) {
            if (!PLAIN_NAME.test(name)) throw invalidName(name);
        }
        return addressFromHost(kind, key) as Address<A>;
    },
    equals(a: Address, b: Address): boolean {
        return a.kind === b.kind && a.key === b.key;
    },
} as const;

/** Internal: an address the host made, so not checked again. */
export function addressFromHost(kind: string, key: string): Address {
    // The brand is type-only.
    return Object.freeze({ kind, key }) as Address;
}

/**
 * Internal: equal in group, code, message, metadata, `public` and
 * `statusCode` to the error `JsAddress::checked` gives for `to`.
 */
export function invalidName(name: string): ActorError {
    const reason = `\`${name}\` is not a valid actor kind or key: use lower-case letters, digits, \`-\`, \`_\` and \`.\`, and do not start with \`.\``;
    return new ActorError(
        "napi",
        "invalid_argument",
        `Invalid native argument 'to': ${reason}`,
        { metadata: { argument: "to", reason } },
    );
}

/** Who made a call, in `hamlet_host::Caller`'s serde form. */
export type Caller = { readonly actor: Address } | "client";

/**
 * Internal: the caller in a call's raw connection parameters, the CBOR of
 * `Option<hamlet_host::Caller>`.
 */
export function callerOf(stamp: unknown): Caller | null {
    if (stamp === null || stamp === "client") return stamp;
    if (typeof stamp === "object" && stamp !== null && "actor" in stamp) {
        const actor: unknown = stamp.actor;
        if (
            typeof actor === "object" &&
            actor !== null &&
            "kind" in actor &&
            "key" in actor &&
            typeof actor.kind === "string" &&
            typeof actor.key === "string"
        ) {
            return { actor: addressFromHost(actor.kind, actor.key) };
        }
    }
    throw internalError("the call's stamp is not a hamlet_host::Caller", {
        metadata: { stamp },
    });
}

export interface CallOptions {
    /**
     * A whole number of milliseconds from 1 to 2^32 - 1, the callee's start
     * included. Past it the call fails with `actor.action_timed_out`, and
     * its outcome is unknown.
     */
    readonly deadlineMs: number;
}

/** The names a caller may call on an address whose actions are `A`. */
export type ActionName<A> = unknown extends A
    ? string
    : Extract<keyof A, string>;

/** The positional arguments of `A`'s action `N`. */
export type ActionArgs<A, N extends string> = unknown extends A
    ? readonly unknown[]
    : N extends keyof A
      ? A[N] extends (...args: infer P) => unknown
          ? Readonly<P>
          : never
      : never;

/** The answer of `A`'s action `N`. */
export type ActionAnswer<A, N extends string> = unknown extends A
    ? unknown
    : N extends keyof A
      ? A[N] extends (...args: never) => infer R
          ? Awaited<R>
          : never
      : never;

export interface Calls {
    /**
     * Calls the action `name` of the actor at `to` as this actor. The answer
     * is decoded with `decodeCborJsonCompat`. Rejects with an `ActorError`
     * for every failure the call reports. Nothing calls again.
     *
     * Before anything is sent, and with the callee running nothing, it
     * rejects instead with the encoder's error for an argument it cannot
     * encode (a `TypeError` for a function or a symbol, a `RangeError` for
     * a cycle), and with napi's `Error` or `TypeError` for an address or
     * options of the wrong JavaScript type, which only a cast lets through.
     */
    call<A, N extends ActionName<A>>(
        to: Address<A>,
        name: N,
        args: ActionArgs<A, N>,
        options: CallOptions,
    ): Promise<ActionAnswer<A, N>>;
}

export interface ContextCalls extends Calls {
    /** This actor's address. A call of it fails with `actor.invalid_operation`. */
    readonly address: Address;
}

export interface ActionCaller {
    /**
     * `null` for the program's own call and for a fire of this actor's own
     * schedule.
     */
    readonly caller: Caller | null;
}

/**
 * The top-level actions of the actor definition `D` as a caller sees them:
 * without their context parameter, their answer awaited.
 */
export type ActionsOf<D> =
    D extends ActorDefinition<any, any, any, any, any, any, any, any, infer R>
        ? {
              readonly [N in keyof R & string as R[N] extends (
                  ...args: never
              ) => unknown
                  ? N
                  : never]: R[N] extends (c: never, ...args: infer P) => infer T
                  ? (...args: P) => Awaited<T>
                  : never;
          }
        : never;
