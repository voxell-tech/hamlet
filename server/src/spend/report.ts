import type { Context } from "@earendil-works/chord";
import type { Home } from "../home.ts";
import { type Limits, LimitsDoc, limitsOf } from "../limits/rules.ts";
import type { Report } from "../protocol.ts";
import { RecordDoc } from "../record.ts";
import { choose, SettingsDoc, settingsOf } from "../settings.ts";
import { type Books, booksOf } from "./ledger.ts";
import { type Amount, type Caps, meters, micros, money } from "./money.ts";

function reached(spent: Readonly<Amount>, caps: Caps): string | undefined {
    const meter = meters(spent, caps).find(
        (m) => m.cap !== null && m.spent >= m.cap,
    );
    return meter === undefined
        ? undefined
        : `${meter.show(meter.spent)} of ${meter.show(meter.cap ?? 0)}`;
}

/** The budget or task cap that stops the next model call (H8, H12). */
export function stopped(
    limits: Readonly<Limits>,
    books: Books,
): string | undefined {
    const hamlet = reached(books.hamlet, limits.budget);
    if (hamlet !== undefined) {
        const period = limits.budget.period === "day" ? "today" : "this month";
        const again =
            limits.budget.period === "day"
                ? "at midnight"
                : "on the first of next month";
        return `The hamlet has spent its budget for ${period} (${hamlet}). It starts again ${again}. Only the chief can raise it in the hamlet's limits.`;
    }
    const task = reached(books.task, limits.task);
    return task === undefined
        ? undefined
        : `This task has spent its cap (${task}). The chief can give the rest of the work as a new task, or raise the cap under One Task in the hamlet's limits.`;
}

/** Builds the chief's spend view with the nearest cap and per-agent amounts. */
export function report(
    limits: Readonly<Limits>,
    books: Books,
    agents: readonly { number: number; name: string; model: string }[],
    now: number,
): Report {
    const nearest = meters(books.hamlet, limits.budget)
        .filter((m) => m.cap !== null)
        .sort(
            (a, b) =>
                b.spent / Math.max(b.cap ?? 0, 1) -
                a.spent / Math.max(a.cap ?? 0, 1),
        )[0];
    const periodWords = limits.budget.period === "day" ? "today" : "this month";
    const summary = `Hamlet ${periodWords}: ${nearest === undefined ? money(books.hamlet.micros) : `${nearest.show(nearest.spent)} of ${nearest.show(nearest.cap ?? 0)}`}`;
    const date = new Date(now);
    const day =
        (date.getHours() * 3600 + date.getMinutes() * 60 + date.getSeconds()) /
        86_400;
    const days = new Date(date.getFullYear(), date.getMonth() + 1, 0).getDate();
    const gone =
        Math.floor(
            (limits.budget.period === "day"
                ? day
                : (date.getDate() - 1 + day) / days) * 200,
        ) / 200;
    return {
        period: limits.budget.period,
        periodWords,
        gone,
        spent: { ...books.hamlet },
        caps: {
            micros:
                limits.budget.cost === null ? null : micros(limits.budget.cost),
            ms:
                limits.budget.minutes === null
                    ? null
                    : limits.budget.minutes * 60_000,
        },
        summary,
        lines: agents.map(
            (agent) =>
                `- ${agent.name}: ${agent.model}, ${money(books.agents[String(agent.number)]?.micros ?? 0)}`,
        ),
        left:
            nearest === undefined
                ? null
                : Math.max(
                      0,
                      1 - nearest.spent / Math.max(nearest.cap ?? 0, 1),
                  ),
        remains:
            nearest === undefined
                ? null
                : nearest.show(Math.max(0, (nearest.cap ?? 0) - nearest.spent)),
    };
}

/** The hamlet's current report, mayor first, with an optional label for its default model. */
export async function reportOf(
    home: Home,
    hamlet: string,
    context: Context,
    fallback?: string,
    counted?: Books,
): Promise<Report> {
    const limits = limitsOf(
        await home.harness.snapshot(LimitsDoc, hamlet, context),
    );
    const books =
        counted ??
        (await booksOf(home, hamlet, limits.budget.period, null, context));
    const record =
        (await home.harness.snapshot(RecordDoc, hamlet, context)) ??
        RecordDoc.definition.initial(hamlet);
    const settings = settingsOf(
        await home.harness.snapshot(SettingsDoc, context),
        home.overrides,
    );
    const agents = [...record.agents]
        .sort(
            (a, b) =>
                Number(b.number === record.mayor) -
                Number(a.number === record.mayor),
        )
        .map((agent) => ({
            number: agent.number,
            name: agent.number === record.mayor ? "mayor" : agent.name,
            model: choose(
                limits,
                { ...settings, model: fallback ?? settings.model },
                {
                    name: agent.name,
                    mayor: agent.number === record.mayor,
                    model: agent.model,
                },
            ),
        }));
    const result = report(limits, books, agents, home.now());
    for (const [key, spent] of Object.entries(books.agents)) {
        const [of, number] = key.split(":");
        if (of === undefined || number === undefined) continue;
        const source = await home.harness.snapshot(RecordDoc, of, context);
        const agent =
            source?.agents.find((a) => a.number === Number(number)) ??
            source?.archive.findLast((a) => a.agent.number === Number(number))
                ?.agent;
        const name =
            agent !== undefined && agent.number === source?.mayor
                ? "mayor"
                : (agent?.name ?? `#${number}`);
        result.lines.push(
            `- ${name}@${of}: ${money(spent.micros)} (paid for another hamlet)`,
        );
    }
    return result;
}

export { roadAfford } from "./ledger.ts";
