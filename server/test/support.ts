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
    type Models,
} from "@earendil-works/pi-ai/models";
import { createFauxCore } from "@earendil-works/pi-ai/providers/faux";
import type {
    Conversation,
    ConversationId,
    EntryRecord,
    Harness,
    Storage,
    StorageWrite,
} from "@earendil-works/pi-durable";
import type { ExecutionEnv } from "@earendil-works/pi-durable/env";
import { openNodeSqliteStorage } from "@earendil-works/pi-durable/storage/sqlite/node";
import { afterEach } from "vitest";

export const context = BACKGROUND_CONTEXT;

const scratches: string[] = [];
const closers: (() => void)[] = [];

afterEach(() => {
    for (const close of closers.splice(0)) close();
    for (const dir of scratches.splice(0)) {
        rmSync(dir, { recursive: true, force: true });
    }
});

/** Calls each of `closers` after the test. */
export function closeAfter(...close: (() => void)[]): void {
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
    models: Models;
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

/** What one request showed the model. */
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
        for (const tool of message.toolsAdded ?? []) tools.add(tool.name);
        for (const tool of message.toolsRemoved ?? []) tools.delete(tool.name);
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

/** What a command printed, both streams in order, and how it ended. */
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
