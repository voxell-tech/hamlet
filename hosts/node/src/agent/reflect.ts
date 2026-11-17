/**
 * Reflection (C5): after every `reflect_every` chief turns in a session,
 * and when a chief leaves the session or asks, the agent is asked to keep
 * what it learned. Reflection `k` of a conversation is admitted under
 * `reflection:<conversation>:<k>`, so a repeat that asks the same one
 * admits it once.
 */
import type { AssistantMessage } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    type Cursor,
    defineExtension,
    defineTask,
    type EntryId,
    type Extension,
    GenerationTask,
    hook,
    runEndTask,
    type Tx,
} from "@earendil-works/pi-durable";
import { withHarness } from "../pi/runtime.ts";
import { type AgentCall, generationOf } from "./actor.ts";
import { hasWordsOrCall, isShortCutOff } from "./compaction.ts";
import type { ReadCopies } from "./copies.ts";
import type { RealmTask } from "./messages.ts";
import { cutOff, draftOf, type Peers, type WithHarness } from "./turn.ts";

/** As `crates/hamlet_server/src/talk/reflections.rs:18-24` asks it. */
export const REFLECT =
    "Reflection time (automatic, not from a chief). Look back over this session. Keep what is worth remembering about the chiefs and the lessons learned in your memory/ folder, and save or improve a skill in skills/ for anything you did that is likely to come up again. If nothing is worth keeping, say so in one line.";

/** `crates/hamlet_server/src/settings.rs:85`. */
const EVERY = 10;

/**
 * A conversation's reflections: `answered`, the chief turns answered since
 * the last one ran, `running` when a chief turn has not ended, and `k`, the
 * number the next is asked under; when `asked`, reflection `k` is owed or
 * running already.
 */
export type Owed = (conversation: ConversationId) => Promise<{
    readonly answered: number;
    readonly running: boolean;
    readonly k: number;
    readonly asked: boolean;
}>;

export function reflection(
    peers: Peers,
    copies: ReadCopies,
    owed: Owed,
    every: number | undefined,
): Extension {
    const asking = reflectTask(peers);
    return defineExtension({
        name: "hamlet.reflection",
        tasks: [asking],
        hooks: [
            hook(GenerationTask, {
                afterRun: async (_run, api) => {
                    const after =
                        every ??
                        (await copies()).settings?.reflect_every ??
                        EVERY;
                    if (after === 0) return undefined;
                    const { answered, k, asked } = await owed(
                        api.conversationId,
                    );
                    if (asked || answered < after) return undefined;
                    return { tasks: [runEndTask(asking, { k })] };
                },
            }),
        ],
    });
}

/**
 * Asks the reflection a session owes as a chief leaves it, by `/new` or
 * `/resume`, whatever `reflect_every` is.
 */
export async function reflectOnLeaving(
    c: AgentCall,
    conversation: ConversationId,
): Promise<void> {
    const { answered, running } = await owedOf((use) =>
        withHarness(c, generationOf(c).options, use),
    )(conversation);
    if (answered >= 1 || running) await reflectIn(c, conversation);
}

/**
 * Asks `conversation` to reflect by the task `hamlet.reflect`, whose memo
 * keeps a repeat after a kill from asking the realm twice. `already` when
 * one is owed or running.
 */
export function reflectIn(
    c: AgentCall,
    conversation: ConversationId,
): Promise<"asked" | "already"> {
    const { peers, options } = generationOf(c);
    return withHarness(c, options, (harness, context) =>
        harness.commit(async (tx) => {
            const { k, asked } = await lastAsked(tx, conversation);
            if (asked) return "already";
            await tx.createTask(
                reflectTask(peers),
                { k },
                {
                    ownership: { kind: "conversation" },
                    conversationId: conversation,
                },
            );
            return "asked";
        }, context),
    );
}

export function owedOf(withHarness: WithHarness): Owed {
    return (conversation) =>
        withHarness(async (harness, context) => {
            const last = await harness.commit(
                (tx) => lastAsked(tx, conversation),
                context,
            );
            const none = { answered: 0, running: false, ...last };
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
            // An aborted turn leaves its line open and its run ended.
            const running =
                open &&
                (await harness.commit(
                    (tx) =>
                        live(tx, conversation, GenerationTask.definition.name),
                    context,
                ));
            return { answered, running, ...last };
        });
}

/**
 * As `crates/hamlet_server/src/talk/verdicts.rs:55-60` rules: an answer
 * with neither words nor a call fails its turn.
 */
function stands(answer: AssistantMessage): boolean {
    return (
        (answer.stopReason === "stop" || answer.stopReason === "length") &&
        hasWordsOrCall(answer)
    );
}

/**
 * The reflection to ask next, and the entry of the last one that ran;
 * `asked` when reflection `k` is admitted and has not ended, or its task
 * has not admitted it yet.
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

/** Whether the newest task of `kind` in `conversation` has not ended. */
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
async function opened(peers: Peers): Promise<RealmTask | undefined> {
    try {
        const answer = await peers.reach.open_task({ turn: "reflection" });
        if (answer.type === "opened") return answer.task;
        peers.report("a reflection's task was refused", answer.words);
    } catch (error) {
        peers.report("a reflection's task was not opened", error);
    }
    return undefined;
}

const ASKED = "asked";

const TASK = "hamlet.reflect";

function reflectTask(peers: Peers) {
    return defineTask<{ k: number }, { phase: "admit" }, null>({
        name: TASK,
        version: 1,
        initial: () => ({ phase: "admit" }),
        phases: {
            admit: async (task, runtime, context) => {
                // `open_task` carries no request id, so a call a kill cut
                // off may have opened a task, and its rerun asks no more.
                const cut =
                    (await runtime.memo<boolean>(ASKED, context)) === true;
                if (!cut) await runtime.memo(ASKED, true, context);
                const realmTask = cut
                    ? undefined
                    : await cutOff(runtime.signal, opened(peers));
                const conversation = await runtime.conversation(
                    runtime.conversationId,
                    context,
                );
                await runtime.commit(async (tx) => {
                    await conversation?.submit(
                        tx,
                        admission(
                            runtime.conversationId,
                            task.input.k,
                            realmTask,
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
