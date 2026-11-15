/**
 * What the agent's tests with no host share: a fake of `Calls`, a fake
 * of the agent's filesystem, and one agent created and woken on a plain
 * `node:sqlite` file with faux models. The agent is the kind's own
 * definition: its migration, `onCreate`, `onWake`, `onDestroy` and actions,
 * as `agentActor` returns them. Only the host is missing.
 */
import type { TestContext } from "node:test";
import type { Context } from "@earendil-works/chord/context";
import {
    type AssistantMessage,
    createAssistantMessageEventStream,
    type FauxResponseStep,
    fauxAssistantMessage,
    fauxToolCall,
    type Models,
} from "@earendil-works/pi-ai";
import type { Harness, HarnessSettings } from "@earendil-works/pi-durable";
import { err, FileError, ok } from "@earendil-works/pi-durable/env";
import {
    ActorError,
    Address,
    type Caller,
    type Calls,
} from "../src/actor/actor/mod.ts";
import type { RawAccess } from "../src/actor/db/mod.ts";
import {
    type AgentCall,
    type AgentConfig,
    type AgentContext,
    type AgentHandlers,
    agentActor,
} from "../src/agent/actor.ts";
import type { AgentFilesystem } from "../src/agent/filesystem.ts";
import {
    type AgentInput,
    REALM,
    type RealmActions,
    WORKSPACE,
    type WorkspaceActions,
} from "../src/agent/messages.ts";
import { createPiRuntime, PI_RUNTIME, piRuntime } from "../src/pi/runtime.ts";
import { context, fauxModels, plainDatabase, scratchFile } from "./testing.ts";

/** One call the agent made. */
export interface Sent {
    readonly to: string;
    readonly name: string;
    readonly message: unknown;
    readonly deadlineMs: number;
}

type Answers<A> = {
    [N in keyof A]?: A[N] extends (...args: infer P) => infer R
        ? (...args: P) => R | Promise<R>
        : never;
};

/**
 * A fake of `Calls`. It records every call and answers from `answers`.
 * The realm allows every model request and takes every charge unless told
 * otherwise. An action with no answer fails as the host's does.
 */
export function fakeCalls(answers: {
    workspace?: Answers<WorkspaceActions>;
    realm?: Answers<RealmActions>;
}): { readonly calls: Calls; readonly sent: Sent[] } {
    const sent: Sent[] = [];
    const takers: Record<
        string,
        Record<string, (...args: never[]) => unknown>
    > = {
        [WORKSPACE.kind]: {
            list_tools: () => ({ tools: [], faults: [] }),
            ...answers.workspace,
        },
        [REALM.kind]: {
            may_spend: () => ({ type: "done" }),
            charge: () => null,
            ...answers.realm,
        },
    };
    const calls: Calls = {
        call: async (to, name, args, options) => {
            sent.push({
                to: `${to.kind}/${to.key}`,
                name,
                message: (args as readonly unknown[])[0],
                deadlineMs: options.deadlineMs,
            });
            const take = takers[to.kind]?.[name];
            if (take === undefined) {
                throw new ActorError(
                    "actor",
                    "action_not_found",
                    `no action ${name}`,
                );
            }
            return (await take(...(args as never[]))) as never;
        },
    };
    return { calls, sent };
}

/** The calls of `sent` to `name`. */
export function named(sent: readonly Sent[], name: string): Sent[] {
    return sent.filter((call) => call.name === name);
}

/** The error of a call past its deadline, whose outcome is unknown. */
export function timedOut(): ActorError {
    return new ActorError("actor", "action_timed_out", "The action timed out.");
}

/** A fake of the agent's filesystem over a map of files, rooted where agent 7's is. */
export function fakeFiles(files: Record<string, string>): AgentFilesystem & {
    readonly edits: number;
} {
    const root = "/realm/actors/agent/7.fs";
    const missing = (path: string) =>
        new FileError("not_found", `ENOENT: ${path}`, path);
    let edits = 0;
    return {
        id: `hamlet-agent-filesystem:${root}`,
        root,
        get edits() {
            return edits;
        },
        read: async (path) => {
            const text = files[path];
            return text === undefined
                ? err(missing(path))
                : ok(new TextEncoder().encode(text));
        },
        write: async (path, content) => {
            const created = files[path] === undefined;
            files[path] =
                typeof content === "string"
                    ? content
                    : new TextDecoder().decode(content);
            return ok({ created });
        },
        edit: async (path, changes) => {
            const before = files[path];
            if (before === undefined) return err(missing(path));
            let text = before;
            for (const change of changes) {
                if (!text.includes(change.old)) {
                    return err(
                        new FileError(
                            "invalid",
                            `\`${change.old}\` is not in ${path}`,
                            path,
                        ),
                    );
                }
                text = text.replace(change.old, change.new);
            }
            files[path] = text;
            edits += 1;
            return ok({ before, after: text });
        },
        list: async () => ok([]),
        search: async () => ok({ found: [], more: false, capped: false }),
        find: async () => ok({ found: [], more: false, capped: false }),
        copy: async () => ok(undefined),
    };
}

/** `models`, with every answer costing `dollars`: the faux provider prices nothing. */
export function priced(models: Models, dollars: number): Models {
    const costing = (message: AssistantMessage): AssistantMessage => ({
        ...message,
        usage: {
            ...message.usage,
            cost: { ...message.usage.cost, total: dollars },
        },
    });
    return new Proxy(models, {
        get(target, key, receiver) {
            if (key === "streamSimple") {
                return (...args: Parameters<Models["streamSimple"]>) => {
                    const out = createAssistantMessageEventStream();
                    void (async () => {
                        const source = target.streamSimple(...args);
                        for await (const event of source) {
                            out.push(
                                event.type === "done"
                                    ? {
                                          ...event,
                                          message: costing(event.message),
                                      }
                                    : event,
                            );
                        }
                        out.end(costing(await source.result()));
                    })();
                    return out;
                };
            }
            const value: unknown = Reflect.get(target, key, receiver);
            return typeof value === "function" ? value.bind(target) : value;
        },
    });
}

const HAND: AgentInput = {
    kind: "hand",
    task: "standing-7",
    place: { version: 1, hires: [], roads: [], tools_off: [] },
};

/** A caller the host stamped: the realm, the workspace, or another actor. */
export const from = {
    realm: { actor: REALM } as Caller,
    workspace: { actor: WORKSPACE } as Caller,
    alarm: null,
    agent: (n: number): Caller => ({ actor: Address.of("agent", String(n)) }),
};

/** The kind's definition, as `actor()` keeps it: what the host calls. */
interface Definition {
    readonly db: { onMigrate(db: RawAccess): Promise<void> };
    onCreate(c: AgentContext, input: unknown): Promise<void>;
    onWake(c: AgentContext): Promise<void>;
    onDestroy(c: AgentContext): Promise<void>;
    readonly actions: AgentHandlers;
}

export interface Woken {
    /** The wake's context. */
    readonly c: AgentContext;
    /** An action's context, with the caller the host stamped. */
    by(caller: Caller | null): AgentCall;
    /** The definition's own actions. */
    readonly actions: AgentHandlers;
    readonly config: AgentConfig;
    readonly faux: ReturnType<typeof fauxModels>["faux"];
    /** The alarms the wrapper asked the host for, by time. */
    readonly alarms: number[];
    /** What the agent logged as a failure that left the turn as it was. */
    readonly reported: string[];
    harness(): Promise<Harness>;
    /** Waits until no conversation has a run. */
    idle(): Promise<void>;
    /** Every entry of the root conversation, oldest first, as its kind, data and text. */
    entries(): Promise<{ kind: string; text: string; data: unknown }[]>;
    /** Makes the database's next transaction fail once, as a full disk would. Resolves when it has. */
    failNextCommit(): Promise<void>;
    /** Closes this wake as the definition's `onDestroy` does: running work stops at once and resumes at the next wake. */
    stop(): Promise<void>;
    /** A new wake of the same agent on the same file. */
    again(): Promise<Woken>;
}

/**
 * One agent, created and woken on a scratch SQLite file through the kind's
 * own definition, with no host under it.
 */
export async function wakeAgent(
    t: TestContext,
    given: {
        calls: Calls;
        files?: AgentFilesystem;
        responses?: FauxResponseStep[];
        input?: AgentInput;
        models?: (models: Models) => Models;
        settings?: HarnessSettings;
        fill?: AgentConfig["fill"];
        key?: string;
    },
): Promise<Woken> {
    const commits = failing(plainDatabase(scratchFile(t)));
    const { faux, models, model } = fauxModels();
    faux.setResponses(given.responses ?? []);
    const files = given.files ?? fakeFiles({});
    const config: AgentConfig = {
        models: given.models === undefined ? models : given.models(models),
        model,
        files: () => files,
        ...(given.fill === undefined ? {} : { fill: given.fill }),
        ...(given.settings === undefined ? {} : { settings: given.settings }),
    };
    // The config is untyped once `actor()` has it, as the host sees it.
    const definition = agentActor(config).config as unknown as Definition;
    await definition.db.onMigrate(commits.database);
    const address = Address.of("agent", given.key ?? "7");
    const start = async (first: boolean): Promise<Woken> => {
        const alarms: number[] = [];
        const reported: string[] = [];
        const c = {
            actorId: `agent-${address.key}`,
            address,
            call: given.calls.call,
            db: commits.database,
            vars: { [PI_RUNTIME]: createPiRuntime() },
            conns: new Map(),
            log: {
                warn: (line: { msg: string; error?: unknown }) =>
                    reported.push(`${line.msg}: ${String(line.error)}`),
                error: (line: { msg: string }) => reported.push(line.msg),
            },
            keepAwake: (promise: Promise<unknown>) => promise,
            schedule: {
                at: async (at: number) => {
                    alarms.push(at);
                },
            },
            abortSignal: new AbortController().signal,
        } as unknown as AgentContext;
        if (first) await definition.onCreate(c, given.input ?? HAND);
        await definition.onWake(c);
        const harness = async () => {
            const open = await piRuntime(c).harness;
            if (open === undefined) throw new Error("the harness is not open");
            return open.harness;
        };
        return {
            c,
            by: (caller) => ({ ...c, caller }) as AgentCall,
            actions: definition.actions,
            config,
            faux,
            alarms,
            reported,
            harness,
            idle: async () => (await harness()).waitForIdle(context),
            entries: async () => entriesOf(await harness(), context),
            failNextCommit: commits.failNext,
            stop: () => definition.onDestroy(c),
            again: () => start(false),
        };
    };
    return start(true);
}

/** `database`, whose next transaction can be made to fail once. */
function failing(database: RawAccess): {
    readonly database: RawAccess;
    failNext(): Promise<void>;
} {
    let armed: (() => void) | undefined;
    return {
        database: {
            ...database,
            transaction: ((...args: never[]) => {
                const fail = armed;
                if (fail !== undefined) {
                    armed = undefined;
                    fail();
                    return Promise.reject(new Error("disk I/O error"));
                }
                return (
                    database.transaction as (
                        ...args: never[]
                    ) => Promise<unknown>
                )(...args);
            }) as RawAccess["transaction"],
        },
        failNext: () => {
            const fired = Promise.withResolvers<void>();
            armed = fired.resolve;
            return fired.promise;
        },
    };
}

async function entriesOf(harness: Harness, context: Context) {
    const root = await harness.root(context);
    const page = await root.entries(
        { order: "ascending" },
        500,
        undefined,
        context,
    );
    return page.items.map((entry) => ({
        kind: entry.kind,
        data: entry.data,
        text: (entry.model ?? [])
            .flatMap((message) =>
                typeof message.content === "string"
                    ? [message.content]
                    : message.content.map((part) =>
                          part.type === "text" ? part.text : `<${part.type}>`,
                      ),
            )
            .join(""),
    }));
}

/** Waits until `check` holds, or fails after five seconds. */
export async function until(
    check: () => boolean | Promise<boolean>,
): Promise<void> {
    const end = Date.now() + 5_000;
    while (!(await check())) {
        if (Date.now() > end)
            throw new Error("waited five seconds for nothing");
        await new Promise((resolve) => setTimeout(resolve, 20));
    }
}

/** `promise`'s outcome, or `words` once `ms` have passed with none. */
export function within<T>(
    promise: Promise<T>,
    ms: number,
    words: string,
): Promise<T | string> {
    return Promise.race([
        promise,
        new Promise<string>((resolve) =>
            setTimeout(() => resolve(words), ms).unref(),
        ),
    ]);
}

/** An answer of the faux model that calls `name`, under the model's call id `id`. */
export function toolCall(
    name: string,
    args: Parameters<typeof fauxToolCall>[1],
    id = "c1",
): AssistantMessage {
    return fauxAssistantMessage([fauxToolCall(name, args, { id })], {
        stopReason: "toolUse",
    });
}
