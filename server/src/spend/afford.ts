import type { Context } from "@earendil-works/chord";
import type {
    ConversationId,
    DocumentReader,
} from "@earendil-works/pi-durable";
import { payerOf } from "../chats/errands.ts";
import { turnOf } from "../chats/turn.ts";
import type { Home } from "../home.ts";
import { LimitsDoc, limitsOf } from "../limits/rules.ts";
import { err, ok, type Result } from "../result.ts";
import { booksOf } from "./ledger.ts";
import { duration, meters, money } from "./money.ts";

/** A reached cost or model-time cap before the next call (H8, H12). */
export type SpendFailure = {
    operation: "spend";
    hamlet: string;
    task: string | null;
    retrySafe: true;
    spent: number;
    cap: number;
    meter: "cost" | "time";
    period: "day" | "month";
} & ({ kind: "budget-spent" } | { kind: "task-spent" });

/** Checks the current payer and turn; explicit overrides select a prospective errand. */
export async function canSpend(
    home: Home,
    read: DocumentReader & { readonly conversationId: ConversationId },
    given: { hamlet?: string; task?: string | null },
    context: Context,
): Promise<Result<void, SpendFailure>> {
    const hamlet =
        given.hamlet ?? (await payerOf(read, read.conversationId, context));
    if (hamlet === undefined) return ok(undefined);
    const task =
        given.task !== undefined
            ? given.task
            : ((await turnOf(home.harness, read, context))?.task ?? null);
    const limits = limitsOf(await read.snapshot(LimitsDoc, hamlet, context));
    const books = await booksOf(
        home,
        hamlet,
        limits.budget.period,
        task,
        context,
    );
    for (const [kind, amount, caps] of [
        ["budget-spent", books.hamlet, limits.budget],
        ["task-spent", books.task, limits.task],
    ] as const) {
        const reached = meters(amount, caps).find(
            (m) => m.cap !== null && m.spent >= m.cap,
        );
        if (reached?.cap != null)
            return err({
                kind,
                operation: "spend",
                hamlet,
                task,
                retrySafe: true,
                spent: reached.spent,
                cap: reached.cap,
                meter: reached.show === money ? "cost" : "time",
                period: limits.budget.period,
            });
    }
    return ok(undefined);
}

/** The cap and the chief's recovery options, in the turn's audience's words. */
export function spendFailureText(error: SpendFailure): string {
    const show =
        error.meter === "cost"
            ? money
            : (n: number) => `${duration(n)} model time`;
    const reached = `${show(error.spent)} of ${show(error.cap)}`;
    switch (error.kind) {
        case "budget-spent": {
            const period = error.period === "day" ? "today" : "this month";
            const again =
                error.period === "day"
                    ? "at midnight"
                    : "on the first of next month";
            return `The hamlet has spent its budget for ${period} (${reached}). It starts again ${again}. Only the chief can raise it in the hamlet's limits.`;
        }
        case "task-spent":
            return `This task has spent its cap (${reached}). The chief can give the rest of the work as a new task, or raise the cap under One Task in the hamlet's limits.`;
    }
    const unhandled: never = error;
    throw new Error(`Unclassified spend failure: ${unhandled}`);
}
