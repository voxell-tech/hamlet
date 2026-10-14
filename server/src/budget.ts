import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import {
    defineDoc,
    defineExtension,
    type Extension,
    GenerationTask,
    type Harness,
    hook,
    type UsageState,
} from "@earendil-works/pi-durable";
import { untilAborted } from "./abort.ts";
import { NoticeEntry } from "./notice.ts";

/** A chief's limits on the hamlet (H22). */
export type Limits = {
    /** Dollars the hamlet may spend in a period (H8). */
    cost: number;
};

export const LimitsDoc = defineDoc<Limits>({
    kind: "hamlet.limits",
    version: 1,
    scope: "session",
    initial: () => ({ cost: 5 }),
});

/** Stops a turn before a model call once the hamlet's budget has run out (H8). */
export function budget(harness: Harness): Extension {
    return defineExtension({
        name: "hamlet.budget",
        hooks: [
            hook(GenerationTask, {
                beforeRequest: async (_request, api, context) => {
                    const limits =
                        (await api.snapshot(LimitsDoc, context)) ??
                        LimitsDoc.definition.initial();
                    if (cost(await harness.usage(context)) < limits.cost) {
                        return undefined;
                    }
                    await harness.commit(async (tx) => {
                        await tx.appendEntry(NoticeEntry, api.conversationId, {
                            data: {
                                text: `The hamlet's budget of $${limits.cost} has run out. A chief can raise it in the hamlet's limits, or start the period again.`,
                            },
                        });
                    }, context);
                    // Not awaited: aborting joins this invocation, which ends
                    // only once the abort signals it.
                    void harness.abortTask(api.taskId, BACKGROUND_CONTEXT);
                    return untilAborted(context);
                },
            }),
        ],
    });
}

function cost(usage: UsageState): number {
    return [...Object.values(usage.models), ...Object.values(usage.tools)]
        .map((spend) => spend.cost.total)
        .reduce((sum, total) => sum + total, 0);
}
