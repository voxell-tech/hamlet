import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import type { Context } from "@earendil-works/chord";
import type { OAuthAuth } from "@earendil-works/pi-ai";
import type { MutableModels } from "@earendil-works/pi-ai/models";
import {
    createRegistry,
    DEFAULT_COMPACTION_POLICY,
    Harness,
    type HarnessSettings,
    type Registry,
    type Storage,
} from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { CharacterDoc } from "./character.ts";
import { Credentials } from "./credentials.ts";
import { closeEnvironments, envFor } from "./environment.ts";
import { agentKey, HOME, WorkDoc } from "./hamlet.ts";
import { buildHouse, type Layout, layOut } from "./layout.ts";
import { LimitsDoc } from "./limits.ts";
import { claim } from "./lock.ts";
import { homeModels } from "./models.ts";
import { PARTS, type Part } from "./parts.ts";
import { RecordDoc } from "./record.ts";
import { platformSandbox, type Sandbox } from "./sandbox/sandbox.ts";
import { type Overrides, readOverrides, SettingsDoc } from "./settings.ts";
import { closeSkills, refreshSkills } from "./skills.ts";

export type HomeOptions = {
    /** pi-ai's models; by default the home's own, from its settings and credentials. */
    readonly models?: MutableModels;
    /** Parts to install after the home's own, in order. */
    readonly parts?: readonly Part[];
    /** The clock; `Date.now` by default. */
    readonly now?: () => number;
    /** Sign-in flows by pi-ai provider id, in place of pi-ai's own. */
    readonly logins?: Readonly<Record<string, OAuthAuth>>;
    /** The environment variables to read; `process.env` by default. */
    readonly env?: Readonly<Record<string, string | undefined>>;
    /** The chief the server runs for; by default `HAMLET_USER`, else `USER`, else `user`. */
    readonly chief?: string;
    /** Opens the home's storage file; SQLite by default. */
    readonly openStorage?: (file: string) => Promise<Storage>;
};

/** A home before its storage opens. */
export type HomeBase = {
    /** The home's folder, as an absolute path. */
    readonly dir: string;
    readonly layout: Layout;
    readonly sandbox: Sandbox;
    readonly chief: string;
    readonly overrides: Overrides;
    readonly credentials: Credentials;
    readonly logins: Readonly<Record<string, OAuthAuth>>;
    now(): number;
};

/** A home this process holds open (S2, ADR 0005). */
export type Home = HomeBase & {
    readonly harness: Harness;
    readonly registry: Registry;
    readonly models: MutableModels;
    /** Room kept free for an answer, in tokens, which compaction keeps (C28). */
    readonly compaction: { reserveTokens: number };
    /** Stops every part, then closes the storage and lets the home go. */
    close(context: Context): Promise<void>;
};

/** Opens the home in `dir`, founding it when it is fresh, and carries on the work it left unfinished (S3). */
export async function openHome(
    dir: string,
    options: HomeOptions,
    context: Context,
): Promise<Home> {
    const root = resolve(dir);
    const env = options.env ?? process.env;
    const overrides = readOverrides(env);
    await mkdir(root, { recursive: true });
    const lock = await claim(dir);
    let opened: Harness | undefined;
    try {
        const layout = await layOut(root);
        const base: HomeBase = {
            dir: root,
            layout,
            sandbox: platformSandbox(),
            chief: options.chief || env.HAMLET_USER || env.USER || "user",
            overrides,
            credentials: await Credentials.open(
                layout.credentials,
                overrides.apiKey,
            ),
            logins: options.logins ?? {},
            now: options.now ?? Date.now,
        };
        const models = options.models ?? homeModels(base);
        const registry = createRegistry();
        const compaction = {
            reserveTokens: DEFAULT_COMPACTION_POLICY.reserveTokens,
        };
        const openStorage = options.openStorage ?? openNodeSqliteStorage;
        const harness = await Harness.open(
            await openStorage(layout.storage),
            {
                models,
                registry,
                settings: runSettings(compaction),
                env: (target, context) =>
                    envFor(base, target, { net: false }, context),
                now: base.now,
            },
            context,
        );
        opened = harness;
        // A storage that failed closes the harness by itself.
        const released = harness.closed.then(async () => {
            await Promise.all([
                closeEnvironments(base, context),
                closeSkills(base.layout.dir, context),
                lock.release(),
            ]);
        });
        const running: ReturnType<Part>[] = [];
        let closing: Promise<void> | undefined;
        const home: Home = {
            ...base,
            harness,
            registry,
            models,
            compaction,
            close: (context) => {
                closing ??= (async () => {
                    for (const part of running.reverse()) {
                        await part.stop?.(context);
                    }
                    refreshSkills(base.layout.dir);
                    await harness.close(context);
                    await released;
                })();
                return closing;
            },
        };
        await found(home, context);
        running.push(
            ...[...PARTS, ...(options.parts ?? [])].map((part) => part(home)),
        );
        for (const part of running) {
            if (part.extension !== undefined) registry.install(part.extension);
        }
        for (const part of running) await part.start?.(context);
        harness.resume();
        return home;
    } catch (error) {
        await opened?.close(context);
        await lock.release();
        throw error;
    }
}

/** The run settings of every conversation: tools one at a time, Rust's retry waits, and compaction keeping `compaction`'s room. */
function runSettings(compaction: {
    readonly reserveTokens: number;
}): HarnessSettings {
    return {
        toolExecution: "sequential",
        retry: { maxRetries: 3, baseDelayMs: 1000, maxAgentDelayMs: 30_000 },
        stream: { maxRetries: 0 },
        get compaction() {
            return {
                reserveTokens: compaction.reserveTokens,
                backgroundTokens: Math.round(compaction.reserveTokens / 2),
            };
        },
    };
}

/** Founds the first hamlet when the home is fresh, and builds each of its agents' houses. */
async function found(home: Home, context: Context): Promise<void> {
    const numbers = await home.harness.commit(async (tx) => {
        const record = await tx.doc(RecordDoc, HOME, HOME);
        await tx.doc(WorkDoc, HOME, HOME);
        await tx.doc(LimitsDoc, HOME, HOME);
        await tx.doc(SettingsDoc);
        for (const { number } of record.agents) {
            const key = agentKey({ hamlet: HOME, number });
            await tx.doc(CharacterDoc, key, key);
        }
        return record.agents.map((agent) => agent.number);
    }, context);
    for (const number of numbers) {
        await buildHouse(home.layout, { hamlet: HOME, number });
    }
}
