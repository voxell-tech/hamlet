import { randomUUID } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import {
    calculateCost,
    createAssistantMessageEventStream,
    type Message,
    type Model,
    type SimpleStreamOptions,
    type StreamFunction,
} from "@earendil-works/pi-ai";
import {
    createModels,
    createProvider,
    type MutableModels,
} from "@earendil-works/pi-ai/models";
import { createFauxCore } from "@earendil-works/pi-ai/providers/faux";
import type {
    Conversation,
    ConversationId,
    EntryRecord,
    Harness,
    Storage,
    StorageWrite,
    Submission,
} from "@earendil-works/pi-durable";
import type { ExecutionEnv } from "@earendil-works/pi-durable/env";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { afterEach } from "vitest";
import { chatOf, submitLine } from "../src/chats.ts";
import { HOME, WorkDoc } from "../src/hamlet.ts";
import { type Home, type HomeOptions, openHome } from "../src/home.ts";
import { buildHouse } from "../src/layout.ts";
import { LimitsDoc } from "../src/limits.ts";
import { RealmDoc } from "../src/realm.ts";
import { RecordDoc } from "../src/record.ts";
import { OversightDoc, type Wait } from "../src/waits.ts";

export const context = BACKGROUND_CONTEXT;

const scratches: string[] = [];
const closers: (() => unknown)[] = [];

afterEach(async () => {
    for (const close of closers.splice(0).reverse()) await close();
    for (const dir of scratches.splice(0)) {
        rmSync(dir, { recursive: true, force: true });
    }
});

/** Calls each of `closers` after the test, the last given first. */
export function closeAfter(...close: (() => unknown)[]): void {
    closers.push(...close);
}

/** A fresh folder, removed after the test. */
export function scratch(): string {
    const dir = mkdtempSync(join(tmpdir(), "hamlet-test-"));
    scratches.push(dir);
    return dir;
}

/** The scripted provider behind {@link fauxModels}. */
export type Faux = ReturnType<typeof createFauxCore>;

/** pi-ai models served only by a faux provider, costing `cost` dollars per million tokens. */
export function fauxModels(
    ids: string[] = ["faux-1"],
    cost = 0,
): {
    models: MutableModels;
    faux: Faux;
} {
    const faux = createFauxCore({
        models: ids.map((id) => ({
            id,
            cost: { input: cost, output: cost, cacheRead: 0, cacheWrite: 0 },
        })),
    });
    const models = createModels();
    models.setProvider(
        createProvider({
            id: faux.provider,
            auth: {
                apiKey: { name: "Faux", resolve: async () => ({ auth: {} }) },
            },
            models: faux.models,
            api: {
                stream: priced(faux.stream),
                streamSimple: priced(faux.streamSimple),
            },
        }),
    );
    return { models, faux };
}

/** The chief of every {@link testHome}. */
export const CHIEF = "chief";

/** The founder of the first hamlet, its mayor. */
export const MAYOR = { hamlet: HOME, number: 0 };

/**
 * Opens a home in `dir` for one test, and closes it after the test. Its chief is {@link CHIEF}, and its mayor runs
 * `faux::faux-1` unless `options.env` names another model.
 */
export async function testHome(
    dir: string,
    options: HomeOptions,
): Promise<Home> {
    const home = await openHome(
        dir,
        {
            chief: CHIEF,
            ...options,
            env: { HAMLET_MODEL: "faux::faux-1", ...options.env },
        },
        context,
    );
    closeAfter(() => home.close(context));
    return home;
}

/** Sends `text` to the home's mayor as a line `chief` typed, with a fresh id. */
export function say(
    home: Home,
    text: string,
    chief = home.chief,
): Promise<Submission> {
    return submitLine(
        home,
        { agent: MAYOR, chief },
        { id: randomUUID(), text },
        context,
    );
}

/** The waits open for a chief now. */
export async function openWaits(home: Home): Promise<Readonly<Wait>[]> {
    const oversight = await home.harness.snapshot(OversightDoc, context);
    return Object.values(oversight?.waits ?? {});
}

/** The current conversation of the home's chief with its mayor. */
export function mayorChat(home: Home): Promise<Conversation> {
    return chatOf(home, { agent: MAYOR, chief: home.chief }, context);
}

/** Has the first hamlet work on `folder`, in `mode`. */
export async function setFolder(
    home: Home,
    folder: string | null,
    mode: "manual" | "auto" = "manual",
): Promise<void> {
    await home.harness.commit(async (tx) => {
        const work = await tx.doc(WorkDoc, HOME, HOME);
        work.folder = folder;
        work.mode = mode;
    }, context);
}

/** Prices each response from its model's rates, as a real provider does. */
function priced(
    stream: StreamFunction<string, SimpleStreamOptions>,
): StreamFunction<string, SimpleStreamOptions> {
    return (model, context, options) => {
        const source = stream(model, context, options);
        const out = createAssistantMessageEventStream();
        void (async () => {
            for await (const event of source) {
                if (event.type === "done") {
                    event.message.usage.cost = calculateCost(
                        model,
                        event.message.usage,
                    );
                }
                out.push(event);
            }
            out.end(await source.result());
        })();
        return out;
    };
}

/** The model, prompt and tools of one request. */
export type Request = {
    model: string;
    sections: Record<string, string>;
    tools: string[];
};

/** Folds a request's system messages into the prompt and tools in effect. */
export function request(messages: Message[], model: Model<string>): Request {
    const sections: Record<string, string> = {};
    const tools = new Set<string>();
    for (const message of messages) {
        if (message.role !== "system") continue;
        for (const [name, text] of Object.entries(message.sections ?? {})) {
            if (text === null) delete sections[name];
            else sections[name] = text;
        }
        for (const tool of message.toolsRemoved ?? []) tools.delete(tool.name);
        for (const tool of message.toolsAdded ?? []) tools.add(tool.name);
    }
    return { model: model.id, sections, tools: [...tools] };
}

/** The conversation's entries, oldest first. */
export async function transcript(
    conversation: Conversation | ConversationId,
    harness?: Harness,
): Promise<readonly EntryRecord[]> {
    const handle =
        typeof conversation === "number"
            ? await harness?.conversation(conversation, context)
            : conversation;
    if (handle === undefined) throw new Error("no such conversation");
    const page = await handle.entries(
        { order: "ascending" },
        1000,
        undefined,
        context,
    );
    return page.items;
}

/** A storage whose first commit that `crashes` matches fails, as if the process died. */
export function crashing(
    crashes: (writes: readonly StorageWrite[]) => boolean,
): (file: string) => Promise<Storage> {
    return async (file) => {
        const storage = await openNodeSqliteStorage(file);
        let crashed = false;
        const commit: Storage["commit"] = (writes, commitContext) => {
            if (!crashed && crashes(writes)) {
                crashed = true;
                return Promise.reject(new Error("crash"));
            }
            return storage.commit(writes, commitContext);
        };
        return new Proxy(storage, {
            get: (target, property) =>
                property === "commit"
                    ? commit
                    : Reflect.get(target, property, target),
        });
    };
}

/** The output of a command, both streams in order, and how it ended. */
export async function sh(
    env: ExecutionEnv,
    script: string,
): Promise<{ exitCode: number; output: string }> {
    let output = "";
    const result = await env.exec(
        script,
        {
            onOutput: (text) => {
                output += text;
            },
        },
        context,
    );
    return { exitCode: result.ok ? result.value.exitCode : -1, output };
}

/** A living agent added directly to its hamlet for a test. */
export async function addAgent(
    home: Home,
    hamlet: string,
    name: string,
    purpose = "",
): Promise<import("../src/hamlet.ts").AgentRef> {
    const agent = await home.harness.commit(async (tx) => {
        const record = await tx.doc(RecordDoc, hamlet, hamlet);
        const number = record.next++;
        record.agents.push({
            number,
            name,
            purpose,
            since: home.now(),
            model: null,
        });
        return { hamlet, number };
    }, context);
    await buildHouse(home.layout, agent);
    return agent;
}

/** A living hamlet and its mayor added directly for a test. */
export async function addHamlet(
    home: Home,
    id: string,
    mayor: string,
): Promise<import("../src/hamlet.ts").AgentRef> {
    await home.harness.commit(async (tx) => {
        (await tx.doc(RealmDoc)).hamlets.push({
            id,
            title: null,
            at: null,
            chiefs: [home.chief],
            from: null,
            founded: home.now(),
            removed: null,
            erased: false,
        });
        const record = await tx.doc(RecordDoc, id, id);
        const founder = record.agents[0];
        if (founder) founder.name = mayor;
        await tx.doc(WorkDoc, id, id);
        await tx.doc(LimitsDoc, id, id);
    }, context);
    await buildHouse(home.layout, { hamlet: id, number: 0 });
    return { hamlet: id, number: 0 };
}

/** The chief's conversation with an agent. */
export function chatWith(
    home: Home,
    ref: import("../src/hamlet.ts").AgentRef,
): Promise<Conversation> {
    return chatOf(home, { agent: ref, chief: home.chief }, context);
}

/** Per-agent faux scripts selected by the prompt's workspace, with a separate errand queue. */
export function fauxAgents(
    faux: Faux,
    scripts: {
        [
            key: string
        ]: import("@earendil-works/pi-ai/providers/faux").FauxResponseStep[];
    },
): void {
    const queues = Object.fromEntries(
        Object.entries(scripts).map(([key, steps]) => [key, [...steps]]),
    );
    const dispatch: import("@earendil-works/pi-ai/providers/faux").FauxResponseFactory =
        (prompt, options, state, model) => {
            const text = prompt.messages
                .filter((m) => m.role === "system")
                .map((m) => JSON.stringify(m))
                .join("\n");
            const found = /Workspace: [^\n"\\]*\/houses\/([^/]+)\/(\d+)/.exec(
                text,
            );
            const errand = prompt.messages.some(
                (m) => m.role === "system" && m.sections?.instructions != null,
            );
            const key =
                !errand && found ? `${found[1]}:${found[2]}` : "errands";
            const step = queues[key]?.shift();
            if (!step) throw new Error(`No faux response for ${key}`);
            faux.appendResponses([dispatch]);
            return typeof step === "function"
                ? step(prompt, options, state, model)
                : step;
        };
    faux.setResponses([dispatch]);
}
