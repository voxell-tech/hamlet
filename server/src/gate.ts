import type { Context, Draft } from "@earendil-works/chord";
import {
    type ConversationId,
    type DocumentReader,
    defineDoc,
    defineExtension,
    type Extension,
    type Harness,
    hook,
    type JsonObject,
    type Session,
    type ToolHookCall,
    ToolTask,
} from "@earendil-works/pi-durable";
import { untilAborted } from "./abort.ts";
import { WorkDoc } from "./hamlet.ts";

export type Answer = "allow" | "deny";

/** A tool call held for a chief (H11). */
export type Wait = {
    conversationId: ConversationId;
    tool: string;
    arguments: JsonObject;
    reason: string;
    answer: Answer | null;
};

/** What waits for a chief, by the held call's tool task. */
export const OversightDoc = defineDoc<{ waits: Record<string, Wait> }>({
    kind: "hamlet.oversight",
    version: 1,
    scope: "session",
    initial: () => ({ waits: {} }),
});

/** A chief's answer to a wait; the first answer stands. */
export async function answerWait(
    session: Session,
    id: string,
    answer: Answer,
    context: Context,
): Promise<void> {
    await session.commit(async (tx) => {
        const wait = (await tx.doc(OversightDoc)).waits[id];
        if (wait === undefined) throw new Error(`Nothing waits as ${id}`);
        wait.answer ??= answer;
    }, context);
}

/** Holds each tool call the hamlet's rules keep for a chief (C7, H11). */
export function gate(harness: Harness): Extension {
    return defineExtension({
        name: "hamlet.gate",
        hooks: [
            hook(ToolTask, {
                beforeTool: async (call, api, context) => {
                    const reason = await holds(call, api, context);
                    if (reason === undefined) return undefined;
                    // Keyed by the call's task, so a restart that runs this
                    // hook again finds the same wait.
                    const id = String(api.taskId);
                    await harness.commit(async (tx) => {
                        (await tx.doc(OversightDoc)).waits[id] ??= {
                            conversationId: api.conversationId,
                            tool: call.name,
                            arguments: call.arguments as Draft<JsonObject>,
                            reason,
                            answer: null,
                        };
                    }, context);
                    return (await answered(harness, id, context)) === "allow"
                        ? undefined
                        : { block: "A chief refused it." };
                },
            }),
        ],
    });
}

const EDITS = new Set(["write", "edit"]);
const COMMANDS = new Set(["bash"]);

/** Why the call waits for a chief, or `undefined` when it may run. */
async function holds(
    call: ToolHookCall,
    read: DocumentReader,
    context: Context,
): Promise<string | undefined> {
    const work =
        (await read.snapshot(WorkDoc, context)) ?? WorkDoc.definition.initial();
    if (work.folder === null || work.mode !== "manual") return undefined;
    if (EDITS.has(call.name)) {
        return "The hamlet is in manual mode: each edit waits for a chief.";
    }
    if (COMMANDS.has(call.name)) {
        return "The hamlet is in manual mode: each command waits for a chief.";
    }
    return undefined;
}

/** The chief's answer to wait `id`, once there is one. */
async function answered(
    harness: Harness,
    id: string,
    context: Context,
): Promise<Answer> {
    const state = await harness.documentState(OversightDoc, context);
    if (state === undefined) throw new Error(`Nothing waits as ${id}`);
    try {
        return await Promise.race([
            new Promise<Answer>((resolve) => {
                const check = (
                    value: { waits: Record<string, Wait> } | null,
                ) => {
                    const answer = value?.waits[id]?.answer;
                    if (answer !== undefined && answer !== null)
                        resolve(answer);
                };
                check(state.value);
                state.subscribe(check);
            }),
            untilAborted(context),
        ]);
    } finally {
        state.dispose();
    }
}
