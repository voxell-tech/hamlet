import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import type { Context } from "@earendil-works/chord";
import type { OAuthAuth } from "@earendil-works/pi-ai";
import type { MutableModels } from "@earendil-works/pi-ai/models";
import {
    type ConversationId,
    type Cursor,
    createRegistry,
    DEFAULT_COMPACTION_POLICY,
    type Extension,
    Harness,
    type HarnessSettings,
    type Registry,
    type Storage,
} from "@earendil-works/pi-durable";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import {
    AgentConversationsDoc,
    ChatDoc,
    type With,
    WithDoc,
} from "./chats/conversations.ts";
import { agentEnv, Confinement, houseReader, WorkDoc } from "./environment.ts";
import { Credentials } from "./home/credentials.ts";
import { buildHouse, type Layout, layOut } from "./home/layout.ts";
import { claim } from "./home/lock.ts";
import { PARTS } from "./home/parts.ts";
import { Houses } from "./house/skills.ts";
import { PeerStatistics } from "./letters.ts";
import { LimitsDoc } from "./limits/rules.ts";
import { homeModels } from "./models.ts";
import { CharacterDoc } from "./prompt.ts";
import { liveHamlets, RealmDoc } from "./realm/places.ts";
import { agentKey, HOME, RecordDoc } from "./record.ts";
import { platformSandbox, type Sandbox } from "./sandbox.ts";
import { type Secrets, secretsFrom } from "./scrub.ts";
import { type Overrides, readOverrides, SettingsDoc } from "./settings.ts";

/** Overrides for the resources and parts opened with a home. */
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
    readonly confinement: Confinement;
    readonly houses: Houses;
    readonly chief: string;
    readonly overrides: Overrides;
    readonly credentials: Credentials;
    /** Scrubs with current credentials before storage or display. */
    readonly secrets: Secrets;
    readonly logins: Readonly<Record<string, OAuthAuth>>;
    now(): number;
};

/** A home this process holds open (S2, ADR 0005). */
export type Home = HomeBase & {
    readonly harness: Harness;
    readonly registry: Registry;
    readonly models: MutableModels;
    /** Peer counts advanced from this harness's committed entries. */
    readonly peerStatistics: PeerStatistics;
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
        const credentials = await Credentials.open(
            layout.credentials,
            overrides.apiKey,
        );
        const sandbox = platformSandbox();
        const confinement = new Confinement();
        const houses = new Houses(layout, (agent, context) =>
            houseReader({ layout, sandbox }, agent, context),
        );
        const base: HomeBase = {
            dir: root,
            layout,
            sandbox,
            confinement,
            houses,
            chief: options.chief || env.HAMLET_USER || env.USER || "user",
            overrides,
            credentials,
            secrets: secretsFrom(credentials),
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
                    agentEnv(base, target, { net: false }, context),
                now: base.now,
            },
            context,
        );
        opened = harness;
        // A storage that failed closes the harness by itself.
        const released = harness.closed.then(async () => {
            await Promise.all([
                base.confinement.close(context),
                base.houses.close(context),
                lock.release(),
            ]);
        });
        const peerStatistics = new PeerStatistics(harness);
        void harness.closed.then(() => peerStatistics.close());
        const running: ReturnType<Part>[] = [];
        let closing: Promise<void> | undefined;
        const home: Home = {
            ...base,
            harness,
            registry,
            models,
            compaction,
            peerStatistics,
            close: (context) => {
                closing ??= (async () => {
                    for (const part of running.reverse()) {
                        await part.stop?.(context);
                    }
                    base.houses.invalidate(base.layout.dir);
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
        for (const part of running) {
            try {
                await part.start?.(context);
            } catch {
                console.error(
                    `Hamlet could not start ${part.extension?.name ?? "a home part"}. The home is open; inspect its pending work before restarting.`,
                );
            }
        }
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
    const existing: { id: ConversationId; place: Readonly<With> }[] = [];
    let cursor: Cursor | undefined;
    do {
        const page = await home.harness.commit(
            (tx) => tx.scanConversations({}, 100, cursor),
            context,
        );
        for (const { id } of page.items) {
            const place = await home.harness.snapshot(WithDoc, id, context);
            if (place) existing.push({ id, place });
        }
        cursor = page.next;
    } while (cursor !== undefined);
    const agents = await home.harness.commit(async (tx) => {
        const realm = await tx.doc(RealmDoc);
        if (realm.hamlets.length === 0)
            realm.hamlets.push({
                id: HOME,
                title: null,
                at: null,
                chiefs: [],
                from: null,
                founded: home.now(),
                removed: null,
                erased: false,
            });
        await tx.doc(SettingsDoc);
        const agents = [];
        for (const { id } of liveHamlets(realm)) {
            const record = await tx.doc(RecordDoc, id, id);
            await tx.doc(WorkDoc, id, id);
            await tx.doc(LimitsDoc, id, id);
            for (const { number } of record.agents) {
                const agent = { hamlet: id, number };
                const key = agentKey(agent);
                await tx.doc(CharacterDoc, key, key);
                await tx.doc(ChatDoc, key, key);
                agents.push(agent);
            }
        }
        for (const { id, place } of existing) {
            const key = agentKey(place);
            const index = await tx.doc(AgentConversationsDoc, key, key);
            if (!index.ids.includes(id)) index.ids.push(id);
        }
        return agents;
    }, context);
    for (const agent of agents) await buildHouse(home.layout, agent);
}

/**
 * One concept's share of an open home: an extension, installed in the order of the parts, and work it starts once
 * every part is installed and stops before the home closes.
 */
export type Part = (home: Home) => {
    readonly extension?: Extension;
    start?(context: Context): Promise<void>;
    stop?(context: Context): Promise<void>;
};

/** A chief's society or realm command. */
export type Deed = (
    home: Home,
    given: { hamlet: string; rest: string; chief: string },
    context: Context,
) => Promise<string>;
