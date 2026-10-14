import type { Static, TSchema } from "@earendil-works/pi-ai";
import {
    defineDoc,
    defineTool,
    type ToolRegistration,
    type Tx,
} from "@earendil-works/pi-durable";

/** An agent as its hamlet's record keeps it (C9). */
export type AgentRecord = {
    number: number;
    name: string;
    purpose: string;
};

/** A decision of a mayor or a chief, with its reason (H17). */
export type Decision = {
    what: string;
    reason: string;
    at: number;
};

/** The hamlet's record: who lives there and what was decided. */
export const RecordDoc = defineDoc<{
    agents: AgentRecord[];
    decisions: Decision[];
}>({
    kind: "hamlet.record",
    version: 1,
    scope: "session",
    initial: () => ({ agents: [], decisions: [] }),
});

/** What a record tool's call told the model, once its change is made. */
const CallDoc = defineDoc<{ result: string | null }>({
    kind: "hamlet.record-call",
    version: 1,
    scope: "task",
    initial: () => ({ result: null }),
});

/**
 * A tool that changes Hamlet's records (ADR 0005). `change` makes the change
 * in `tx` and returns what the model is told; a throw changes nothing.
 */
export function defineRecordTool<P extends TSchema>(tool: {
    name: string;
    description: string;
    parameters: P;
    change(tx: Tx, args: Static<P>): Promise<string>;
}): ToolRegistration<P> {
    return defineTool({
        name: tool.name,
        description: tool.description,
        parameters: tool.parameters,
        replay: "safe",
        execute: async (args, api, context) => {
            // The change and the marker commit together, so a call cut off
            // before its result runs again and finds the result it gave.
            const text = await api.commit(async (tx) => {
                const call = await tx.doc(CallDoc, api.taskId);
                call.result ??= await tool.change(tx, args);
                return call.result;
            }, context);
            return { output: [{ type: "text", text }] };
        },
    });
}
