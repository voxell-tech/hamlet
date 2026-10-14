import type { Context } from "@earendil-works/chord";
import type { Static, TSchema } from "@earendil-works/pi-ai";
import {
    type DocumentReader,
    defineDoc,
    defineDocFamily,
    defineTool,
    type ToolRegistration,
    type Tx,
} from "@earendil-works/pi-durable";
import { type AgentRef, FOUNDER, MAYOR } from "./hamlet.ts";

/** An agent as its hamlet's record keeps it (C9). */
export type AgentRecord = {
    number: number;
    name: string;
    purpose: string;
    /** Its time of creation; 0 for the founder. */
    since: number;
};

/** A decision of a mayor or a chief, with who made it and why (H17). */
export type Decision =
    | {
          kind: "model";
          at: number;
          by: string;
          /** The agent the model is chosen for, by number. */
          agent: number;
          /** The model it runs from now on, as `endpoint::id` (H29). */
          model: string;
          reason: string;
      }
    | {
          kind: "created";
          at: number;
          by: string;
          agent: number;
          reason: string;
      }
    | {
          kind: "folder_set";
          at: number;
          by: string;
          folder: string | null;
      }
    | {
          kind: "mode_set";
          at: number;
          by: string;
          mode: "manual" | "auto";
      }
    | {
          kind: "skill_given";
          at: number;
          by: string;
          to: number;
          skill: string;
          from: string | null;
      };

/** A hamlet's record: who lives there, who holds the office and what was decided. */
export type HamletRecord = {
    agents: AgentRecord[];
    /** The number of the agent that holds the mayor's office. */
    mayor: number;
    decisions: Decision[];
};

/** Each hamlet's record, by hamlet id. A hamlet starts with its founder in the office. */
export const RecordDoc = defineDocFamily<HamletRecord, string>({
    kind: "hamlet.record",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({
        agents: [{ number: 0, name: FOUNDER, purpose: "", since: 0 }],
        mayor: 0,
        decisions: [],
    }),
});

/** The agent `agent` names in `record`: `mayor` for whoever holds the office, otherwise by name. */
export function named(
    record: Readonly<HamletRecord>,
    agent: string,
): AgentRecord | undefined {
    const number = agent === MAYOR ? record.mayor : undefined;
    return record.agents.find((each) =>
        number === undefined ? each.name === agent : each.number === number,
    );
}

/** The address of agent `number` in `record`: `mayor` while it holds the office, otherwise its name. */
export function addressOf(
    record: Readonly<HamletRecord>,
    number: number,
): string {
    if (record.mayor === number) return MAYOR;
    return record.agents.find((each) => each.number === number)?.name ?? "";
}

/** The agent an address names, as {@link named} finds it in its hamlet's record. */
export async function addressed(
    read: DocumentReader,
    address: { hamlet: string; agent: string },
    context: Context,
): Promise<AgentRef | undefined> {
    const record =
        (await read.snapshot(RecordDoc, address.hamlet, context)) ??
        RecordDoc.definition.initial(address.hamlet);
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
