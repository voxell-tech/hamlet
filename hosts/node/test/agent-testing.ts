import assert from "node:assert/strict";
import { copyFileSync, existsSync } from "node:fs";
import type { TestContext } from "node:test";
import type { Context } from "@earendil-works/chord/context";
import {
    type AssistantMessage,
    createAssistantMessageEventStream,
    type FauxResponseFactory,
    type FauxResponseStep,
    fauxAssistantMessage,
    fauxToolCall,
    type Message,
    type Context as ModelContext,
    type Models,
    type ToolResultMessage,
} from "@earendil-works/pi-ai";
import type {
    Harness,
    HarnessSettings,
    ToolDiagnostic,
} from "@earendil-works/pi-durable";
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
import { textOf } from "../src/agent/chat-items.ts";
import type { AgentFilesystem } from "../src/agent/filesystem.ts";
import type { AgentInput, PromptSection } from "../src/agent/messages/agent.ts";
import { LIBRARY, type LibraryActions } from "../src/agent/messages/library.ts";
import type { ProfileActions } from "../src/agent/messages/profile.ts";
import {
    type AgentName,
    type Kept,
    REALM,
    type RealmActions,
} from "../src/agent/messages/realm.ts";
import {
    WORKSPACE,
    type WorkspaceActions,
} from "../src/agent/messages/workspace.ts";
import { REFLECT } from "../src/agent/reflect.ts";
import { conversationOfChat } from "../src/agent/sessions.ts";
import { createPiRuntime, PI_RUNTIME, piRuntime } from "../src/pi/runtime.ts";
import { context, fauxModels, plainDatabase, scratchFile } from "./testing.ts";

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

export function fakeCalls(answers: {
    workspace?: Answers<WorkspaceActions>;
    realm?: Answers<RealmActions>;
    library?: Answers<LibraryActions>;
    /** Every profile's. */
    profile?: Answers<ProfileActions>;
}): { readonly calls: Calls; readonly sent: Sent[] } {
    const sent: Sent[] = [];
    const takers: Record<
        string,
        Record<string, (...args: never[]) => unknown>
    > = {
        [WORKSPACE.kind]: {
            list_tools: () => ({
                tools: [],
                faults: [],
                confinement: { type: "sandbox", name: "bwrap" },
            }),
            cancel: () => ({ commands: [] }),
            ...answers.workspace,
        },
        [REALM.kind]: {
            may_spend: () => ({ type: "done" }),
            charge: () => null,
            read_copies: () => ({}),
            secrets: () => ({ version: 1, values: [] }),
            lapse: () => null,
            turn_failed: () => null,
            working: () => null,
            read_place: () => ({ place: "# Your place\nYou are `hand`." }),
            read_own_place: () => ({
                version: 0,
                hires: [],
                roads: [],
                tools_off: [],
                manual: false,
                agrees_roads: false,
            }),
            ...answers.realm,
        },
        [LIBRARY.kind]: {
            shelves: () => ({ version: 0, skills: [] }),
            ...answers.library,
        },
        profile: { ...answers.profile },
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

export function shown(agent: number): AgentName {
    return { agent, name: `hire#${agent}` };
}

export function named(sent: readonly Sent[], name: string): Sent[] {
    return sent.filter((call) => call.name === name);
}

export function timedOut(): ActorError {
    return new ActorError("actor", "action_timed_out", "The action timed out.");
}

export const FAKE_ROOT = "/realm/actors/agent/7.fs";

export function fakeFiles(files: Record<string, string>): AgentFilesystem & {
    readonly edits: number;
} {
    const root = FAKE_ROOT;
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
            return ok(created);
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
        list: async (path) => {
            const names = new Set(
                Object.keys(files)
                    .filter((name) => name.startsWith(`${path}/`))
                    .map(
                        (name) =>
                            name.slice(path.length + 1).split("/")[0] ?? "",
                    ),
            );
            return ok(
                [...names].sort().map((name) => ({
                    name,
                    path: `${root}/${path}/${name}`,
                    kind:
                        files[`${path}/${name}`] === undefined
                            ? ("directory" as const)
                            : ("file" as const),
                    size: 0,
                })),
            );
        },
        search: async () => ok({ found: [], more: false, capped: false }),
        find: async () => ok({ found: [], more: false, capped: false }),
        copy: async () => ok(undefined),
        walk: async (path) =>
            ok({
                files: Object.keys(files).filter(
                    (name) => path === "" || name.startsWith(`${path}/`),
                ),
                capped: false,
            }),
        remove: async (path) => {
            for (const name of Object.keys(files)) {
                if (name === path || name.startsWith(`${path}/`)) {
                    delete files[name];
                }
            }
            return ok(undefined);
        },
    };
}

/** `fakeFiles` that keeps which files run, as a filesystem with modes does. */
export function filesThatRun(files: Record<string, string>) {
    const fake = fakeFiles(files);
    const runs = new Set<string>();
    const filesystem: AgentFilesystem = {
        ...fake,
        write: async (path, content, context, executable) => {
            if (executable === true) runs.add(path);
            if (executable === false) runs.delete(path);
            return fake.write(path, content, context);
        },
        list: async (path, context) => {
            const listed = await fake.list(path, context);
            if (!listed.ok) return listed;
            return ok(
                listed.value.map((entry) => ({
                    ...entry,
                    executable: runs.has(`${path}/${entry.name}`),
                })),
            );
        },
        walk: async (path, context) => {
            const walked = await fake.walk(path, context);
            if (!walked.ok) return walked;
            const executable = walked.value.files.filter((file) =>
                runs.has(file),
            );
            return ok({ ...walked.value, executable });
        },
    };
    return { filesystem, runs };
}

/** `fakeFiles(files)`, whose walk stops at its cap. */
export function cappedFiles(files: Record<string, string>): AgentFilesystem {
    const filesystem = fakeFiles(files);
    return {
        ...filesystem,
        walk: async (path, context) => {
            const walked = await filesystem.walk(path, context);
            return walked.ok ? ok({ ...walked.value, capped: true }) : walked;
        },
    };
}

/** A model step held until `answer`, or until its request is aborted. */
export function held(): {
    readonly step: FauxResponseStep;
    readonly asked: Promise<readonly Message[]>;
    answer(message: AssistantMessage): void;
} {
    const asked = Promise.withResolvers<readonly Message[]>();
    const answer = Promise.withResolvers<AssistantMessage>();
    return {
        step: (request, options) => {
            asked.resolve(request.messages);
            const aborted = Promise.withResolvers<AssistantMessage>();
            options?.signal?.addEventListener(
                "abort",
                () =>
                    aborted.resolve(
                        fauxAssistantMessage("", {
                            stopReason: "aborted",
                            errorMessage: "aborted",
                        }),
                    ),
                { once: true },
            );
            return Promise.race([answer.promise, aborted.promise]);
        },
        asked: asked.promise,
        answer: answer.resolve,
    };
}

/** Answers `words`, keeping the request it answers in `seen`. */
export function asked(
    seen: ModelContext[],
    words: string,
): FauxResponseFactory {
    return (context) => {
        seen.push(context);
        return fauxAssistantMessage(words);
    };
}

export function userWords(context: ModelContext): string[] {
    return context.messages.flatMap((message) =>
        message.role !== "user"
            ? []
            : typeof message.content === "string"
              ? [message.content]
              : message.content.flatMap((part) =>
                    part.type === "text" ? [part.text] : [],
                ),
    );
}

/** Each answer costs `dollars`, and uses `tokens` when they are given. */
export function priced(
    models: Models,
    dollars: number,
    tokens?: { readonly input: number; readonly output: number },
): Models {
    const costing = (message: AssistantMessage): AssistantMessage => ({
        ...message,
        usage: {
            ...message.usage,
            ...tokens,
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
    place: {
        version: 1,
        hires: [],
        roads: [],
        tools_off: [],
        manual: false,
        agrees_roads: false,
    },
    copies: {},
};

export const from = {
    realm: { actor: REALM } as Caller,
    workspace: { actor: WORKSPACE } as Caller,
    library: { actor: LIBRARY } as Caller,
    profile: (n: number): Caller => ({
        actor: Address.of("profile", String(n)),
    }),
    client: "client" as Caller,
    alarm: null,
    agent: (n: number): Caller => ({ actor: Address.of("agent", String(n)) }),
};

interface Definition {
    readonly db: { onMigrate(db: RawAccess): Promise<void> };
    onCreate(c: AgentContext, input: unknown): Promise<void>;
    onWake(c: AgentContext): Promise<void>;
    onDestroy(c: AgentContext): Promise<void>;
    readonly actions: AgentHandlers;
}

export interface Woken {
    readonly c: AgentContext;
    by(caller: Caller | null): AgentCall;
    readonly actions: AgentHandlers;
    readonly config: AgentConfig;
    readonly faux: ReturnType<typeof fauxModels>["faux"];
    readonly alarms: number[];
    readonly reported: string[];
    /** The events the agent broadcast, in order, as `[name, ...args]`. */
    readonly events: unknown[][];
    /** How many times the agent asked its host to destroy it. */
    readonly destroyed: number;
    harness(): Promise<Harness>;
    idle(): Promise<void>;
    entries(): Promise<{ kind: string; text: string; data: unknown }[]>;
    /** Makes the database's next transaction fail once. Resolves when it has. */
    failNextCommit(): Promise<void>;
    /**
     * Holds the agent's next read of `sql`, outside a transaction, until
     * `release`; `reached` resolves when it is made.
     */
    holdRead(sql: string): Held;
    /**
     * As `onDestroy` does: running work stops at once and resumes at the
     * next wake.
     */
    stop(): Promise<void>;
    again(): Promise<Woken>;
    /**
     * As a kill does: wakes the agent on what its file holds now, and stops
     * this one, whose later writes the new one never sees.
     */
    killed(): Promise<Woken>;
}

/**
 * Answers the reflection a `/new` or a `/resume` asks "Kept.", and every
 * other request from `answers` in order, whichever conversation asks first.
 */
export function reflecting(answers: string[]): FauxResponseStep[] {
    const step: FauxResponseStep = (request) => {
        const last = request.messages.at(-1);
        const text =
            typeof last?.content === "string"
                ? last.content
                : (last?.content ?? [])
                      .map((part) => (part.type === "text" ? part.text : ""))
                      .join("");
        return fauxAssistantMessage(
            text === REFLECT ? "Kept." : (answers.shift() ?? "Nothing left."),
        );
    };
    return Array.from({ length: answers.length + 2 }, () => step);
}

export function joined(sections: readonly PromptSection[]): string {
    return sections.map(({ text }) => text).join("\n\n");
}

export async function readPrompt(agent: Woken): Promise<string> {
    return joined(
        (await agent.actions.read_prompt(agent.by(from.client))).sections,
    );
}

export async function notices(
    agent: Woken,
): Promise<{ text: string; at: number }[]> {
    return (await agent.entries())
        .filter((entry) => entry.kind === "hamlet.notice")
        .map((entry) => entry.data as { text: string; at: number });
}

export async function wakeAgent(
    t: TestContext,
    given: {
        calls: Calls;
        files?: AgentFilesystem;
        responses?: FauxResponseStep[];
        input?: AgentInput;
        models?: (models: Models) => Models;
        settings?: HarnessSettings;
        library?: string;
        key?: string;
    },
): Promise<Woken> {
    const file = scratchFile(t);
    const commits = failing(plainDatabase(file));
    const { faux, models, model } = fauxModels();
    faux.setResponses(given.responses ?? []);
    const files = given.files ?? fakeFiles({});
    const config: AgentConfig = {
        models: given.models === undefined ? models : given.models(models),
        model,
        files: () => files,
        ...(given.library === undefined ? {} : { library: given.library }),
        ...(given.settings === undefined ? {} : { settings: given.settings }),
    };
    // The config is untyped once `actor()` has it, as the host sees it.
    const definition = agentActor(config).config as unknown as Definition;
    await definition.db.onMigrate(commits.database);
    const address = Address.of("agent", given.key ?? "7");
    const start = async (
        first: boolean,
        file: string,
        commits: ReturnType<typeof failing>,
    ): Promise<Woken> => {
        const alarms: number[] = [];
        const reported: string[] = [];
        const events: unknown[][] = [];
        let destroyed = 0;
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
            broadcast: (name: string, ...args: unknown[]) =>
                events.push([name, ...args]),
            schedule: {
                at: async (at: number) => {
                    alarms.push(at);
                },
            },
            abortSignal: new AbortController().signal,
            destroy: () => {
                destroyed += 1;
            },
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
            events,
            get destroyed() {
                return destroyed;
            },
            harness,
            idle: async () => (await harness()).waitForIdle(context),
            entries: async () => entriesOf(await harness(), context),
            failNextCommit: commits.failNext,
            holdRead: commits.holdRead,
            stop: () => definition.onDestroy(c),
            again: () => start(false, file, commits),
            killed: async () => {
                const copy = scratchFile(t);
                for (const suffix of ["", "-journal"]) {
                    if (existsSync(file + suffix)) {
                        copyFileSync(file + suffix, copy + suffix);
                    }
                }
                await definition.onDestroy(c);
                return start(false, copy, failing(plainDatabase(copy)));
            },
        };
    };
    return start(true, file, commits);
}

export interface Held {
    readonly reached: Promise<void>;
    release(): void;
}

function failing(database: RawAccess): {
    readonly database: RawAccess;
    failNext(): Promise<void>;
    holdRead(sql: string): Held;
} {
    let armed: (() => void) | undefined;
    const holds = new Map<
        string,
        { reach(): void; readonly released: Promise<void> }
    >();
    const held =
        <F extends (sql: string, ...args: never[]) => Promise<unknown>>(
            read: F,
        ) =>
        async (sql: string, ...args: never[]) => {
            const hold = holds.get(sql);
            if (hold !== undefined) {
                holds.delete(sql);
                hold.reach();
                await hold.released;
            }
            return read(sql, ...args);
        };
    return {
        database: {
            ...database,
            get: held(database.get) as RawAccess["get"],
            all: held(database.all) as RawAccess["all"],
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
        holdRead: (sql) => {
            const reached = Promise.withResolvers<void>();
            const released = Promise.withResolvers<void>();
            holds.set(sql, {
                reach: reached.resolve,
                released: released.promise,
            });
            return { reached: reached.promise, release: released.resolve };
        },
    };
}

async function rootEntries(harness: Harness, context: Context) {
    const root = await harness.root(context);
    const page = await root.entries(
        { order: "ascending" },
        500,
        undefined,
        context,
    );
    return page.items;
}

async function entriesOf(harness: Harness, context: Context) {
    return (await rootEntries(harness, context)).map((entry) => ({
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

export interface ToolResult {
    /** The text before the diagnostics pi-durable renders after it. */
    readonly output: string;
    readonly isError: boolean;
    readonly diagnostics: readonly ToolDiagnostic[];
    readonly details?: unknown;
}

/** Today's `tool_interrupted` (`crates/hamlet_server/src/calls.rs`). */
export function interrupted(tool: string): ToolResult {
    return {
        output: "",
        isError: true,
        diagnostics: [
            {
                severity: "error",
                code: "tool_interrupted",
                message: `Tool ${tool} was interrupted by a restart and may have partially run`,
            },
        ],
    };
}

export async function toolResults(
    agent: Pick<Woken, "harness">,
): Promise<ToolResult[]> {
    return (await rootEntries(await agent.harness(), context))
        .filter((entry) => entry.kind === "pi.tool-result")
        .map((entry) => {
            const message = entry.model?.[0] as ToolResultMessage;
            const { diagnostics } = entry.data as {
                diagnostics: ToolDiagnostic[];
            };
            const output = message.content
                .slice(0, diagnostics.length > 0 ? -1 : undefined)
                .map((part) => (part.type === "text" ? part.text : "<image>"))
                .join("");
            return {
                output,
                isError: message.isError,
                diagnostics,
                ...(message.details === undefined
                    ? {}
                    : { details: message.details }),
            };
        });
}

export async function until(
    check: () => boolean | Promise<boolean>,
    ms = 5_000,
): Promise<void> {
    const end = Date.now() + ms;
    while (!(await check())) {
        if (Date.now() > end)
            throw new Error(`waited ${ms / 1000} seconds for nothing`);
        await new Promise((resolve) => setTimeout(resolve, 20));
    }
}

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

/** A model call that fails with `words`. */
export function modelFailed(words: string): AssistantMessage {
    return fauxAssistantMessage([], {
        stopReason: "error",
        errorMessage: words,
    });
}

export function toolCall(
    name: string,
    args: Parameters<typeof fauxToolCall>[1],
    id = "c1",
): AssistantMessage {
    return fauxAssistantMessage([fauxToolCall(name, args, { id })], {
        stopReason: "toolUse",
    });
}

/**
 * A realm whose `credential` answers what `keep_credential` kept, and whose
 * secret list holds every secret of it.
 */
export function credentialsFile() {
    const file = new Map<string, Kept>();
    const { calls, sent } = fakeCalls({
        realm: {
            credential: ({ provider }) =>
                file.get(provider) ?? {
                    type: "refused",
                    why: { type: "not_signed_in", provider },
                    words: `Sign in to ${provider} first.`,
                },
            keep_credential: ({ provider, value }) => {
                file.set(provider, value);
                return null;
            },
            secrets: () => ({
                version: 1,
                values: [...file.values()].flatMap((kept) =>
                    kept.type === "key"
                        ? [kept.key]
                        : [kept.login.access, kept.login.refresh],
                ),
            }),
        },
    });
    return { file, calls, sent };
}

/** The item id a client reads for the chief's line `text` in the current session. */
export async function itemOfLine(agent: Woken, text: string): Promise<string> {
    const id = await conversationOfChat(agent.by(from.client), { with: 7 });
    assert.ok(id !== undefined, "the chief has a current session");
    const page = await (
        await (await agent.harness()).conversation(id, context)
    )?.entries({ order: "ascending" }, 500, undefined, context);
    const line = page?.items.find(
        (entry) => entry.kind === "hamlet.line" && textOf(entry) === text,
    );
    assert.ok(line !== undefined, `the line ${text} is in the session`);
    return `${line.id}:0`;
}

/** Every table of the agent's database, as text. */
export async function dumped(agent: Woken): Promise<string> {
    const tables = await agent.c.db.all<{ name: string }>(
        "SELECT name FROM sqlite_master WHERE type = 'table'",
    );
    const decoder = new TextDecoder();
    let text = "";
    for (const { name } of tables) {
        const rows = await agent.c.db.all<Record<string, unknown>>(
            `SELECT * FROM "${name}"`,
        );
        text += JSON.stringify(rows, (_key, value: unknown) =>
            value instanceof Uint8Array ? decoder.decode(value) : value,
        );
    }
    return text;
}
