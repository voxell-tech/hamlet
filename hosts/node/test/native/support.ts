import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import type { CoreRegistry } from "@hamlet/actor-node";
import type { AnyActorDefinition } from "../../src/actor/actor/definition.ts";
import {
    type ActionAnswer,
    type ActionArgs,
    type ActionName,
    type ActionsOf,
    Address,
    type CallOptions,
    type Calls,
} from "../../src/actor/actor/mod.ts";
import type { JsonCompatValue } from "../../src/actor/common/encoding.ts";

type Kinds = Readonly<Record<string, AnyActorDefinition>>;

export interface LocalHost<K extends Kinds> extends Calls {
    /** The actors the host started at once: an alarm was due, or a kill cut off their run. */
    readonly due: readonly string[];
    at<N extends Extract<keyof K, string>>(
        kind: N,
        key: string,
    ): Address<ActionsOf<K[N]>>;
    create(to: Address, input?: JsonCompatValue): Promise<void>;
    getOrStart(to: Address): Promise<void>;
    shutdown(): Promise<void>;
}

/**
 * Starts `kinds` on the native module's host in this process, each actor in
 * its own file at `actors/<kind>/<key>.sqlite` under `dir`. Needs the native
 * module built and copied beside its loader in `crates/hamlet_actor_node`.
 */
export async function startLocalHost<K extends Kinds>(
    dir: string,
    kinds: K,
): Promise<LocalHost<K>> {
    // Loaded here, not at the top: the layer's registry is large, and every
    // test that starts no host, kill children included, would load it.
    const [
        { RegistryConfigSchema },
        { buildNativeRegistry, callNative, callThrough },
        { encodeCborCompat },
    ] = await Promise.all([
        import("../../src/actor/registry/config/index.ts"),
        import("../../src/actor/registry/native.ts"),
        import("../../src/actor/serde.ts"),
    ]);
    const { registry } = await buildNativeRegistry(
        RegistryConfigSchema.parse({ use: kinds }),
    );
    const native = registry as unknown as CoreRegistry;
    const due = await callNative(() => native.start(dir));
    return {
        due,
        at: (kind, key) => Address.of(kind, key),
        create: async (to, input = null) => {
            await callNative(() =>
                native.create(to, Buffer.from(encodeCborCompat(input))),
            );
        },
        getOrStart: async (to) => {
            await callNative(() => native.getOrStart(to));
        },
        async call<A, N extends ActionName<A>>(
            to: Address<A>,
            name: N,
            args: ActionArgs<A, N>,
            options: CallOptions,
        ) {
            return (await callThrough(
                (bytes) => native.call(to, name, Buffer.from(bytes), options),
                args,
            )) as ActionAnswer<A, N>;
        },
        shutdown: () => callNative(() => native.shutdown()),
    };
}

export async function inFolder<T>(
    body: (dir: string) => Promise<T>,
): Promise<T> {
    const dir = mkdtempSync(join(tmpdir(), "hamlet-node-host-"));
    try {
        return await body(dir);
    } finally {
        rmSync(dir, { recursive: true, force: true });
    }
}

export async function eventually<T>(
    read: () => Promise<T>,
    done: (value: T) => boolean,
    ms = 5_000,
): Promise<T> {
    const until = Date.now() + ms;
    for (;;) {
        const value = await read();
        if (done(value)) return value;
        if (Date.now() > until) {
            throw new Error(
                `not done after ${ms} ms: ${JSON.stringify(value)}`,
            );
        }
        await sleep(50);
    }
}
