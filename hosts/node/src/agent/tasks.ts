/**
 * The inputs given under one of the realm's tasks and when each ended, so
 * the realm can ask whether the agent works on a chief's task before a
 * lead ends or merges its hire, as `protects` of
 * `crates/hamlet_server/src/talk/tasks.rs` asks.
 */
import type { ConversationId, Harness } from "@earendil-works/pi-durable";
import type { RawAccess } from "../actor/db/mod.ts";
import { withHarness } from "../pi/runtime.ts";
import { type AgentCall, type AgentContext, generationOf } from "./actor.ts";
import type { OnTask } from "./messages/agent.ts";
import type { RealmTask } from "./messages/shared.ts";

export async function migrateTasks(
    raw: Pick<RawAccess, "execute">,
): Promise<void> {
    await raw.execute(`
        CREATE TABLE IF NOT EXISTS task_input (
            conversation INTEGER NOT NULL,
            request TEXT NOT NULL,
            task TEXT NOT NULL,
            ended_at INTEGER,
            PRIMARY KEY (conversation, request)
        ) STRICT
    `);
}

/** Keeps the input admitted as `request` in `conversation` under `task`. */
export async function given(
    c: Pick<AgentContext, "db">,
    conversation: ConversationId,
    request: string,
    task: RealmTask,
): Promise<void> {
    await c.db.run(
        "INSERT OR IGNORE INTO task_input (conversation, request, task) VALUES (?, ?, ?)",
        conversation,
        request,
        task,
    );
}

const followed = new WeakSet<Harness>();

/** Keeps the time each input of `harness` ends from now on. */
export function followEnds(
    c: Pick<AgentContext, "db" | "log">,
    harness: Harness,
): void {
    if (followed.has(harness)) return;
    followed.add(harness);
    harness.observeCommits({
        advance: ({ changes }) => {
            const at = Date.now();
            for (const change of changes) {
                if (
                    change.type !== "submission" ||
                    change.value.type !== "input" ||
                    change.value.requestId === undefined ||
                    (change.value.status !== "done" &&
                        change.value.status !== "unanswered")
                ) {
                    continue;
                }
                const { conversationId, requestId } = change.value;
                // An observer may not call the harness, nor wait.
                queueMicrotask(() => {
                    ended(c, conversationId, requestId, at).catch(
                        (error: unknown) =>
                            c.log.warn({
                                msg: "an input's end time was not kept",
                                error,
                            }),
                    );
                });
            }
        },
        end: () => {},
    });
}

function ended(
    c: Pick<AgentContext, "db">,
    conversation: ConversationId,
    request: string,
    at: number,
): Promise<unknown> {
    return c.db.run(
        "UPDATE task_input SET ended_at = ? WHERE conversation = ? AND request = ? AND ended_at IS NULL",
        at,
        conversation,
        request,
    );
}

export async function on_task(
    c: AgentCall,
    { tasks, within_ms }: OnTask,
): Promise<boolean> {
    if (tasks.length === 0) return false;
    const now = Date.now();
    const rows = await c.db.all<{
        conversation: ConversationId;
        request: string;
        ended_at: number | null;
    }>(
        `SELECT conversation, request, ended_at FROM task_input WHERE task IN (${tasks.map(() => "?").join(", ")})`,
        ...tasks,
    );
    for (const row of rows) {
        let endedAt = row.ended_at;
        if (endedAt === null) {
            const status = await withHarness(
                c,
                generationOf(c).options,
                (harness, context) =>
                    harness.commit(
                        async (tx) =>
                            (
                                await tx.submissionByRequest(
                                    row.conversation,
                                    row.request,
                                )
                            )?.status,
                        context,
                    ),
            );
            if (status === "queued" || status === "placed") return true;
            // Its end came while no one followed the harness: it counts
            // as ending now, which protects the hire a little longer.
            endedAt = now;
            await ended(c, row.conversation, row.request, now);
        }
        if (now - endedAt < Number(within_ms)) return true;
    }
    return false;
}
