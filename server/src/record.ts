import type { Context } from "@earendil-works/chord";
import type { Static, TSchema } from "@earendil-works/pi-ai";
import {
    type DocumentReader,
    defineDoc,
    defineTool,
    type ToolRegistration,
    type Tx,
} from "@earendil-works/pi-durable";
import { type AgentRef, RecordDoc } from "./record/document.ts";
import { named } from "./record/names.ts";

export {
    type AgentRecord,
    type AgentRef,
    type Archived,
    agentKey,
    type Decision,
    type End,
    FOUNDER,
    type HamletRecord,
    HOME,
    MAYOR,
    RecordDoc,
    type RoadRecord,
} from "./record/document.ts";
export { admit, depart, type Restored, restore } from "./record/membership.ts";
export {
    addressOf,
    byWords,
    checkName,
    endName,
    farewell,
    named,
    nameOf,
    tidyName,
} from "./record/names.ts";
export { requireMayor } from "./record/office.ts";
export { type RecordRefusal, recordRefusalText } from "./record/refusals.ts";
export { carryRoad, hasRoad, level, roadsOf } from "./record/roads.ts";
/** The agent an address names, as {@link named} finds it in its hamlet's record. */
export async function addressed(
    read: DocumentReader,
    address: { hamlet: string; agent: string },
    context: Context,
): Promise<AgentRef | undefined> {
    const record = await read.snapshot(RecordDoc, address.hamlet, context);
    if (record === undefined) return undefined;
    const found = named(record, address.agent);
    return found && { hamlet: address.hamlet, number: found.number };
}

/** The answer a record tool's call gave the model, once its change is made. */
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
