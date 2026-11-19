/**
 * Reflection (C5): after every `reflect_every` chief turns in a session,
 * and when a chief leaves the session or asks, the agent is asked to keep
 * what it learned. Reflection `k` of a conversation is admitted under
 * `reflection:<conversation>:<k>`, so a repeat that asks the same one
 * admits it once.
 */

import { randomUUID } from "node:crypto";
import type { Context } from "@earendil-works/chord/context";
import type { AssistantMessage } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    type Cursor,
    defineExtension,
    defineTask,
    type EntryId,
    type Extension,
    GenerationTask,
    type Harness,
    hook,
    LiveDoc,
    type TaskId,
    type Tx,
} from "@earendil-works/pi-durable";
import { withHarness } from "../pi/runtime.ts";
import { type AgentCall, generationOf } from "./actor.ts";
import { isShortCutOff, stands } from "./compaction.ts";
import type { ReadCopies } from "./copies.ts";
import type { RealmTask } from "./messages.ts";
import {
    cutOff,
    draftOf,
    isBusy,
    type Peers,
    type WithHarness,
} from "./turn.ts";

/** As `crates/hamlet_server/src/talk/reflections.rs:18-24` asks it. */
export const REFLECT =
    "Reflection time (automatic, not from a chief). Look back over this session. Keep what is worth remembering about the chiefs and the lessons learned in your memory/ folder, and save or improve a skill in skills/ for anything you did that is likely to come up again. If nothing is worth keeping, say so in one line.";

/** `crates/hamlet_server/src/settings.rs:85`. */
const EVERY = 10;

/**
 * A conversation's reflections: `answered`, the chief turns answered since
 * the last one ran, and `k`, the number the next is asked under; when
 * `asked`, reflection `k` is owed or running already.
 */
type Owed = (conversation: ConversationId) => Promise<{
    readonly answered: number;
    readonly k: number;
    readonly asked: boolean;
}>;

/**
 * As today's `answered` (`crates/hamlet_server/src/talk/reflections.rs`),
 * a turn answered in a session the chief has left asks its reflection at
 * once, whatever `reflect_every` is.
 */
export function reflection(
    peers: Peers,
    copies: ReadCopies,
    withHarness: WithHarness,
    every: number | undefined,
    chief: () => Promise<ConversationId | undefined>,
): Extension {
    const owed = owedOf(withHarness);
    return defineExtension({
        name: "hamlet.reflection",
        tasks: [reflectTask(peers)],
        hooks: [
            hook(GenerationTask, {
                afterRun: async (_run, api) => {
                    const now = await chief();
                    const left =
                        now !== undefined && now !== api.conversationId;
                    const after =
                        every ??
                        (await copies()).settings?.reflect_every ??
                        EVERY;
                    if (!left && after === 0) return undefined;
                    const { answered, asked } = await owed(api.conversationId);
                    if (asked || answered === 0) return undefined;
                    if (!left && answered < after) return undefined;
                    await withHarness((harness, context) =>
                        ask(harness, peers, api.conversationId, context),
                    );
                    return undefined;
                },
            }),
        ],
    });
}

/**
 * Asks the reflection a session owes as a chief leaves it, by `/new` or
 * `/resume`, whatever `reflect_every` is, and gives its task: `already` when
 * one is owed or running, none when the session owes none. A turn still
 * running then asks its own when it answers.
 */
export async function reflectOnLeaving(
    c: AgentCall,
    conversation: ConversationId,
): Promise<TaskId<null> | "already" | undefined> {
    const { peers, options } = generationOf(c);
    const { answered } = await owedOf((use) => withHarness(c, options, use))(
        conversation,
    );
    if (answered < 1) return undefined;
    return withHarness(c, options, (harness, context) =>
        ask(harness, peers, conversation, context),
    );
}

/**
 * As `reflectOnLeaving`, and an idle session takes its reflection before
 * this ends, so what the chief's move writes next comes after it, as today
 * keeps the reflection first; a busy one takes it when its run ends.
 */
export async function reflectBeforeLeaving(
    c: AgentCall,
    conversation: ConversationId,
): Promise<void> {
    const task = await reflectOnLeaving(c, conversation);
    if (task === undefined || task === "already") return;
    await withHarness(c, generationOf(c).options, async (harness, context) => {
        if (!(await isBusy(harness, conversation, context))) {
            await harness.waitForTask(task, context);
        }
    });
}

/**
 * Asks `conversation` to reflect by the task `hamlet.reflect`, whose memo
 * keeps the request a repeat after a kill asks the realm under. `already`
 * when one is owed or running.
 */
export async function reflectIn(
    c: AgentCall,
    conversation: ConversationId,
): Promise<"asked" | "already"> {
    const { peers, options } = generationOf(c);
    const task = await withHarness(c, options, (harness, context) =>
        ask(harness, peers, conversation, context),
    );
    return task === "already" ? task : "asked";
}

function ask(
    harness: Harness,
    peers: Peers,
    conversation: ConversationId,
    context: Context,
): Promise<TaskId<null> | "already"> {
    return harness.commit(async (tx) => {
        const { k, asked } = await lastAsked(tx, conversation);
        if (asked) return "already";
        return tx.createTask(
            reflectTask(peers),
            { k },
            {
                ownership: { kind: "conversation" },
                conversationId: conversation,
            },
        );
    }, context);
}

function owedOf(withHarness: WithHarness): Owed {
    return (conversation) =>
        withHarness(async (harness, context) => {
            const last = await harness.commit(
                (tx) => lastAsked(tx, conversation),
                context,
            );
            const none = { answered: 0, ...last };
            if (last.asked) return none;
            const kept = await harness.conversation(conversation, context);
            if (kept === undefined) return none;
            let answered = 0;
            // A chief's line or retry is open until the turn answers; lines
            // steered into one turn are one turn.
            let open = false;
            let cursor: Cursor | undefined;
            do {
                const page = await kept.entries(
                    {
                        order: "ascending",
                        ...(last.since === undefined
                            ? {}
                            : { minEntryId: last.since }),
                    },
                    500,
                    cursor,
                    context,
                );
                for (const entry of page.items) {
                    // A fork reads its parent's entries, reflections too,
                    // but none of its parent's submissions.
                    if (entry.kind === "hamlet.reflection") {
                        answered = 0;
                        open = false;
                    }
                    if (CHIEF_TURNS.has(entry.kind)) open = true;
                    if (entry.kind !== "pi.assistant") continue;
                    const answer = entry.model?.[0] as AssistantMessage;
                    // A short cut-off's second try, `hamlet.cut-off`,
                    // answers the same turn.
                    if (
                        answer.stopReason === "toolUse" ||
                        isShortCutOff(answer)
                    )
                        continue;
                    if (open && stands(answer)) answered += 1;
                    open = false;
                }
                cursor = page.next;
            } while (cursor !== undefined);
            return { answered, ...last };
        });
}

/**
 * `asked` when reflection `k` is admitted and has not ended, or its task has
 * not admitted it yet.
 */
async function lastAsked(
    tx: Tx,
    conversation: ConversationId,
): Promise<{ k: number; asked: boolean; since: EntryId | undefined }> {
    let since: EntryId | undefined;
    for (let k = 1; ; k += 1) {
        const asked = await tx.submissionByRequest(
            conversation,
            requestOf(conversation, k),
        );
        if (asked === undefined) {
            return {
                k,
                asked: await live(tx, conversation, TASK),
                since,
            };
        }
        if (asked.status === "queued" || asked.status === "placed") {
            return { k, asked: true, since };
        }
        since = asked.entry ?? since;
    }
}

async function live(
    tx: Tx,
    conversation: ConversationId,
    kind: string,
): Promise<boolean> {
    const [newest] = (
        await tx.scanTasks(
            {
                conversationId: conversation,
                kind,
                order: "descending",
            },
            1,
        )
    ).items;
    return newest !== undefined && newest.state.outcome === undefined;
}

/** `HAMLET_REFLECT_EVERY`; absent, the copy's setting holds. 0 turns it off. */
export function reflectEvery(): number | undefined {
    // As `crates/hamlet_server/src/settings.rs:168-175` reads it: a value
    // that is not a whole number is ignored.
    const every = process.env.HAMLET_REFLECT_EVERY;
    return every !== undefined && /^\+?[0-9]+$/.test(every)
        ? Number(every)
        : undefined;
}

const CHIEF_TURNS = new Set(["hamlet.line", "hamlet.retry"]);

function requestOf(conversation: ConversationId, k: number): string {
    return `reflection:${conversation}:${k}`;
}

function admission(
    conversation: ConversationId,
    k: number,
    task: RealmTask | undefined,
) {
    return draftOf({
        kind: "reflection",
        id: requestOf(conversation, k),
        content: REFLECT,
        ...(task === undefined ? {} : { task }),
    });
}

/**
 * The realm's task of its own for a reflection (H12). Without one, the
 * reflection is still asked, and the standing task pays.
 */
async function opened(
    peers: Peers,
    request: string,
): Promise<RealmTask | undefined> {
    try {
        const answer = await peers.reach.open_task({
            request,
            turn: "reflection",
        });
        if (answer.type === "opened") return answer.task;
        peers.report("a reflection's task was refused", answer.words);
    } catch (error) {
        peers.report("a reflection's task was not opened", error);
    }
    return undefined;
}

const REQUEST = "request";

const TASK = "hamlet.reflect";

type ReflectState = { phase: "admit" } | { phase: "place"; task?: RealmTask };

function reflectTask(peers: Peers) {
    return defineTask<{ k: number }, ReflectState, null>({
        name: TASK,
        version: 1,
        initial: () => ({ phase: "admit" }),
        phases: {
            admit: async (_task, runtime, context) => {
                // A rerun after a kill asks under the same request, so the
                // realm answers the task it opened.
                let request = await runtime.memo<string>(REQUEST, context);
                if (request === undefined) {
                    request = randomUUID();
                    await runtime.memo(REQUEST, request, context);
                }
                const realmTask = await cutOff(
                    runtime.signal,
                    opened(peers, request),
                );
                await runtime.commit(
                    () => ({
                        status: "running",
                        checkpoint: {
                            phase: "place",
                            ...(realmTask === undefined
                                ? {}
                                : { task: realmTask }),
                        },
                    }),
                    context,
                );
            },
            // A busy conversation would place a queued line and the
            // reflection in one run, so the reflection waits for the run.
            place: async (task, runtime, context) => {
                const { checkpoint } = task.state;
                const conversation = await runtime.conversation(
                    runtime.conversationId,
                    context,
                );
                await runtime.commit(async (tx) => {
                    const { run } = await tx.doc(
                        LiveDoc,
                        runtime.conversationId,
                    );
                    if (run !== undefined) {
                        return {
                            status: "waiting",
                            checkpoint,
                            on: [run.taskId],
                            policy: "allSettled",
                        };
                    }
                    await conversation?.submit(
                        tx,
                        admission(
                            runtime.conversationId,
                            task.input.k,
                            checkpoint.phase === "place"
                                ? checkpoint.task
                                : undefined,
                        ),
                    );
                    return {
                        status: "terminal",
                        outcome: { status: "completed", result: null },
                    };
                }, context);
            },
        },
        abort: (_task, runtime, context) =>
            runtime.commit(
                () => ({ status: "terminal", outcome: { status: "aborted" } }),
                context,
            ),
    });
}
