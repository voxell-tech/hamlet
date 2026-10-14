import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import {
    CompactionTask,
    type ConversationId,
    defineDoc,
    defineExtension,
    GenerationTask,
    hook,
    SessionFailed,
    section,
} from "@earendil-works/pi-durable";
import { untilAborted } from "./abort.ts";
import { WithDoc } from "./chats.ts";
import { booksOf, markUncounted, reconcile } from "./ledger.ts";
import { LimitsDoc, limitsOf } from "./limits.ts";
import { notify } from "./notice.ts";
import type { Part } from "./parts.ts";
import { reportOf, stopped } from "./report.ts";
import { turnOf } from "./turn.ts";

const WordsDoc = defineDoc<{ input: number | null; words: string }>({
    kind: "hamlet.budget-words",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "initial",
    initial: () => ({ input: null, words: "" }),
});

/** The budget check, spend receipts and the last, turn-fixed prompt section (H8, C11). */
export const Budget: Part = (home) => {
    let unsubscribe: (() => void) | undefined;
    let pending = Promise.resolve();
    return {
        extension: defineExtension({
            name: "hamlet.budget",
            sections: [
                section(
                    "budget",
                    async (input, context) => {
                        const place = await input.read.snapshot(
                            WithDoc,
                            input.conversationId,
                            context,
                        );
                        if (place === undefined) return undefined;
                        const turn = await turnOf(
                            home.harness,
                            {
                                snapshot: input.read.snapshot.bind(input.read),
                                snapshotAsOf: input.read.snapshotAsOf.bind(
                                    input.read,
                                ),
                                conversationId: input.conversationId,
                            },
                            context,
                        );
                        const saved = await input.read.snapshot(
                            WordsDoc,
                            input.conversationId,
                            context,
                        );
                        if (turn !== undefined && saved?.input === turn.input)
                            return saved.words;
                        const report = await reportOf(
                            home,
                            place.hamlet,
                            context,
                            "the hamlet's own model",
                        );
                        const words = `# Budget\nBudget: you and every agent spend from the hamlet's one budget, and only the chief can change it.\n${[report.summary, ...report.lines].join("\n")}`;
                        if (turn !== undefined)
                            await home.harness.commit(async (tx) => {
                                Object.assign(
                                    await tx.doc(
                                        WordsDoc,
                                        input.conversationId,
                                    ),
                                    { input: turn.input, words },
                                );
                            }, context);
                        return words;
                    },
                    { tag: false },
                ),
            ],
            hooks: [
                hook(GenerationTask, {
                    beforeRequest: async (_request, api, context) => {
                        const place = await api.snapshot(
                            WithDoc,
                            api.conversationId,
                            context,
                        );
                        if (place === undefined) return undefined;
                        const turn = await turnOf(home.harness, api, context);
                        const limits = limitsOf(
                            await api.snapshot(
                                LimitsDoc,
                                place.hamlet,
                                context,
                            ),
                        );
                        const reason = stopped(
                            limits,
                            await booksOf(
                                home,
                                place.hamlet,
                                limits.budget.period,
                                turn?.task ?? null,
                                context,
                            ),
                        );
                        if (reason === undefined) {
                            await markUncounted(
                                home,
                                api.conversationId,
                                context,
                            );
                            return undefined;
                        }
                        await home.harness.commit(
                            (tx) =>
                                notify(
                                    tx,
                                    api.conversationId,
                                    reason,
                                    home.now(),
                                ),
                            context,
                        );
                        void home.harness.abortTask(
                            api.taskId,
                            BACKGROUND_CONTEXT,
                        );
                        return untilAborted(context);
                    },
                    afterResponse: async (_message, api, context) =>
                        reconcile(home, context, [api.conversationId]),
                }),
                hook(CompactionTask, {
                    beforeCompact: async (_compaction, api, context) => {
                        const place = await api.snapshot(
                            WithDoc,
                            api.conversationId,
                            context,
                        );
                        if (place === undefined) return undefined;
                        const turn = await turnOf(home.harness, api, context);
                        const limits = limitsOf(
                            await api.snapshot(
                                LimitsDoc,
                                place.hamlet,
                                context,
                            ),
                        );
                        const reason = stopped(
                            limits,
                            await booksOf(
                                home,
                                place.hamlet,
                                limits.budget.period,
                                turn?.task ?? null,
                                context,
                            ),
                        );
                        if (reason !== undefined) return { decline: true };
                        await markUncounted(home, api.conversationId, context);
                        return undefined;
                    },
                }),
            ],
        }),
        async start(context) {
            await reconcile(home, context);
            unsubscribe = home.harness.subscribeCommits(({ changes }) => {
                const conversations = new Set<ConversationId>();
                for (const change of changes) {
                    if (
                        change.type === "document" &&
                        change.record.kind === "pi.usage" &&
                        change.conversationId !== undefined
                    )
                        conversations.add(change.conversationId);
                    if (
                        change.type === "task" &&
                        (change.value.kind === "pi.generation" ||
                            change.value.kind === "pi.compaction") &&
                        change.value.state.status === "terminal"
                    )
                        conversations.add(change.value.conversationId);
                }
                if (conversations.size === 0) return;
                // A listener cannot read on the mutation line it observes.
                pending = pending
                    .then(() =>
                        reconcile(home, BACKGROUND_CONTEXT, [...conversations]),
                    )
                    .catch(() => {});
            });
        },
        async stop(context) {
            unsubscribe?.();
            try {
                await pending;
                await reconcile(home, context);
            } catch (error) {
                if (!(error instanceof SessionFailed)) throw error;
            }
        },
    };
};
