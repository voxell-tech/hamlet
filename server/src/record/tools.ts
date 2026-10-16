import type { Context } from "@earendil-works/chord";
import type { Static, TSchema } from "@earendil-works/pi-ai";
import {
    defineDoc,
    defineTool,
    type ToolExecutionApi,
    type ToolExecutionResult,
    type ToolRegistration,
    type Tx,
} from "@earendil-works/pi-durable";
import { WithDoc } from "../chats/conversations.ts";
import { turnOf } from "../chats/turn.ts";
import type { Home } from "../home.ts";
import type { TreeFailure } from "../house.ts";
import { type Limits, LimitsDoc } from "../limits/rules.ts";
import type { Result } from "../result.ts";
import { reconcile } from "../spend/ledger.ts";
import type { AgentRef, HamletRecord } from "./document.ts";
import {
    type DecisionHeld,
    decisionText,
    mayDecide,
    requireMayor,
} from "./office.ts";
import { type RecordRefusal, recordRefusalText } from "./refusals.ts";

const CallDoc = defineDoc<{ result: string | null }>({
    kind: "hamlet.record-call",
    version: 1,
    scope: "task",
    initial: () => ({ result: null }),
});

/** Commits a change with its call's existing memo shape, returning the saved answer on replay (ADR 0005). */
export function memoised<T>(
    api: ToolExecutionApi,
    step: {
        read(tx: Tx): Promise<T | undefined>;
        change(tx: Tx): Promise<T>;
        write(tx: Tx, value: T): Promise<void>;
    },
    context: Context,
): Promise<T> {
    return api.commit(async (tx) => {
        const saved = await step.read(tx);
        if (saved !== undefined) return saved;
        const value = await step.change(tx);
        await step.write(tx, value);
        return value;
    }, context);
}

/** A replay-safe record tool whose text and record change commit together. */
export function defineRecordTool<P extends TSchema>(tool: {
    name: string;
    description: string;
    parameters: P;
    change(tx: Tx, args: Static<P>): Promise<string>;
}): ToolRegistration<P> {
    return defineTool({
        ...tool,
        replay: "safe",
        execute: async (args, api, context) => {
            const text = await memoised(
                api,
                {
                    read: async (tx) =>
                        (await tx.doc(CallDoc, api.taskId)).result ?? undefined,
                    change: (tx) => tool.change(tx, args),
                    write: async (tx, value) => {
                        (await tx.doc(CallDoc, api.taskId)).result = value;
                    },
                },
                context,
            );
            return { output: [{ type: "text", text }] };
        },
    });
}

/** The tool audience's one translation for record and held-decision refusals. */
export function toolText(failure: RecordRefusal | DecisionHeld): string {
    return failure.kind === "vote-open"
        ? decisionText(failure)
        : recordRefusalText(failure, "agent");
}

/** A mayor's record change checked and memoised in the transaction that makes it. */
export function defineMayorTool<P extends TSchema>(
    home: Home,
    tool: {
        name: string;
        description: string;
        parameters: P;
        change(
            tx: Tx,
            args: Static<P>,
            mayor: {
                place: AgentRef;
                record: HamletRecord;
                limits: Readonly<Limits>;
                task: string | null;
            },
        ): Promise<Result<string, RecordRefusal | DecisionHeld>>;
        after?(
            place: AgentRef,
            args: Static<P>,
            context: Context,
        ): Promise<void>;
    },
): ToolRegistration<P> {
    return defineTool({
        name: tool.name,
        description: tool.description,
        parameters: tool.parameters,
        replay: "safe",
        execute: async (args, api, context) => {
            const place = await api.snapshot(
                WithDoc,
                api.conversationId,
                context,
            );
            if (!place)
                return {
                    output: [
                        { type: "text", text: "Only the mayor may do that." },
                    ],
                };
            const task =
                (await turnOf(home.harness, api, context))?.task ?? null;
            await reconcile(home, context);
            const result = await defineRecordTool({
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
                change: async (tx, args) => {
                    const mayor = await requireMayor(tx, place);
                    if (!mayor.ok) return toolText(mayor.error);
                    const held = await mayDecide(
                        tx,
                        place.hamlet,
                        home.now(),
                        tool.name,
                    );
                    if (!held.ok) return toolText(held.error);
                    const result = await tool.change(tx, args, {
                        place,
                        record: mayor.value,
                        limits: await tx.doc(
                            LimitsDoc,
                            place.hamlet,
                            place.hamlet,
                        ),
                        task,
                    });
                    return result.ok ? result.value : toolText(result.error);
                },
            }).execute(args, api, context);
            await tool.after?.(place, args, context);
            return result;
        },
    });
}

/** A chief's record operation, returning expected refusals for the protocol envelope. */
export type RecordDeed = (
    home: Home,
    given: { hamlet: string; rest: string; chief: string },
    context: Context,
) => Promise<Result<string, RecordRefusal | DecisionHeld | TreeFailure>>;

/** A returned refusal in the model's tool-error diagnostic channel. */
export function refusedTool(text: string): ToolExecutionResult {
    return {
        output: [],
        isError: true,
        diagnostics: [{ severity: "error", code: "tool_error", message: text }],
    };
}
