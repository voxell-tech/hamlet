/**
 * The chief's commands to an agent: what each changes and how it is
 * refused.
 */
import type { Context } from "@earendil-works/chord/context";
import type { AssistantMessage } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    type Cursor,
    type EntryId,
    type Harness,
    InboxDoc,
    LiveDoc,
    type SubmissionDraft,
} from "@earendil-works/pi-durable";
import { withHarness } from "../pi/runtime.ts";
import { type AgentCall, generationOf, shownWithdrawn } from "./actor.ts";
import { textOf } from "./chat-items.ts";
import { stands } from "./compaction.ts";
import { couldNotAnswer } from "./failed.ts";
import type {
    AgentActions,
    ClientActions,
    NewSession,
    Resume,
    ResumeAnswer,
    Retry,
    RetryAnswer,
    Rewind,
    RewindAnswer,
    Stop,
    Withdrawn,
} from "./messages/agent.ts";
import type { LapseReason } from "./messages/realm.ts";
import { type LetterId, u32 } from "./messages/shared.ts";
import {
    reflectBeforeLeaving,
    reflectIn,
    reflectOnLeaving,
} from "./reflect.ts";
import {
    begin,
    chiefSession,
    conversationFor,
    created,
    current,
    makeCurrent,
    moveTo,
    noticeEntry,
    session,
} from "./sessions.ts";
import { timersEnded } from "./timers.ts";
import { holdFollowers, refused } from "./transcript.ts";
import {
    callOf,
    conversationsOf,
    draftOf,
    isBusy,
    isOpen,
    type Kept,
    refOf,
} from "./turn.ts";
import { endsLater } from "./workspace-tools.ts";

type Answer<N extends keyof ClientActions> = Promise<
    ReturnType<ClientActions[N]>
>;

/**
 * The realm's stop of the agent, run once per number; a repeat answers the
 * first count. It counts the conversations it stops and the chats its
 * withdrawn inputs are shown in.
 */
export function stop(c: AgentCall, message: Stop): Promise<Stopped> {
    const { stops } = generationOf(c);
    const first = stops.get(message.number);
    if (first !== undefined) return first;
    const stopping = stopped(c, message.withdrawn ?? []);
    stops.set(message.number, stopping);
    stopping.catch(() => stops.delete(message.number));
    return stopping;
}

type Stopped = ReturnType<AgentActions["stop"]>;

/** Stops every turn, and shows the `lines` the realm withdrew without running them. */
export async function stopped(
    c: AgentCall,
    lines: readonly Withdrawn[],
): Promise<Stopped> {
    const generation = generationOf(c);
    const { options, peers } = generation;
    // Marked first, so an admission that begins later shows the line and
    // never runs it. Not in `inTurn`, which may hold the very run it ends.
    for (const line of lines) generation.withdrawn.add(line.id);
    await Promise.allSettled(generation.admitting);
    // Today runs a chief's commands to one agent one after another
    // (`crates/hamlet_server/src/commands.rs`), so a retry under way is
    // stopped too.
    await generation.retrying;
    const shown = await Promise.all(
        lines.map(async (line) => ({
            line,
            chat: await conversationFor(c, line.line),
        })),
    );
    const ended = await withHarness(c, options, async (harness, context) => {
        const each = await Promise.all(
            (await conversationsOf(harness, context)).map(async (id) => {
                if (!(await isOpen(harness, id, context))) return [];
                const dropped = await droppedQuestions(harness, id, context);
                await (await harness.conversation(id, context))?.abort(context);
                return [{ id, dropped }];
            }),
        );
        for (const { line, chat } of shown) {
            await shownWithdrawn(harness, chat, line.id, line.line, context);
        }
        return each.flat();
    });
    await peers.cancel({ which: { type: "all" } });
    // As today's follow answers them (`crates/hamlet_server/src/talk/follow.rs`).
    for (const question of ended.flatMap((one) => one.dropped)) {
        await couldNotAnswer(
            peers,
            question,
            "its turn was stopped",
            undefined,
        );
    }
    const counted = new Set([
        ...ended.map((one) => one.id),
        ...shown.map((one) => one.chat),
    ]);
    return { turns: counted.size };
}

/**
 * Withdraws each input `conversation` has queued and gives the questions
 * among them; one a run took first is left to that run's end.
 */
async function droppedQuestions(
    harness: Harness,
    conversation: ConversationId,
    context: Context,
): Promise<LetterId[]> {
    const queued = await harness.snapshot(InboxDoc, conversation, context);
    const questions: LetterId[] = [];
    for (const item of queued?.items ?? []) {
        if (item.mode === "write") continue;
        const withdrawn = await harness.abortSubmission(
            item.id,
            context,
            conversation,
        );
        const { question } = (item.entry?.data ?? {}) as Kept;
        if (withdrawn === "aborted" && question !== undefined) {
            questions.push(question);
        }
    }
    return questions;
}

export async function newSession(
    c: AgentCall,
    message: NewSession,
): Answer<"new_session"> {
    const generation = generationOf(c);
    const answered = Promise.withResolvers<void>();
    try {
        const { number, left } = await generation.inTurn(() => {
            // Followers hear of the new session after its answer, once its
            // chat's row is kept: a commit folded before that is never sent.
            holdFollowers(c, answered.promise);
            return begin(c, message.id);
        });
        // As today, the session left keeps what it waits for.
        if (left !== undefined) {
            void reflectOnLeaving(c, left).catch((error: unknown) =>
                generation.peers.report(
                    "the session left was not asked to reflect",
                    error,
                ),
            );
        }
        return { number };
    } finally {
        // The answer is handed to the runtime before the next turn of the loop.
        setImmediate(answered.resolve);
    }
}

export async function resume(
    c: AgentCall,
    message: Resume,
): Promise<ResumeAnswer> {
    const to = await session(c, message.number);
    if (to === undefined) {
        const { number } = message;
        return {
            type: "refused",
            why: {
                type: "no_such_session",
                number: u32(typeof number === "bigint" ? Infinity : number),
            },
            // As `crates/hamlet_server/src/society/refusal.rs` words it.
            words: `there is no past session ${number} here`,
        };
    }
    // A session gone back to asks again what it was trusted with (H11),
    // before it is current, as today resets trust in the move's own
    // transaction. The session left reflects before the move too, so a
    // repeat after a kill still asks it.
    await lapsed(c, to, "resumed");
    await generationOf(c).inTurn(async () => {
        const left = await current(c.db);
        if (left !== undefined && left !== to) {
            await reflectBeforeLeaving(c, left);
        }
        await makeCurrent(c, to);
        await noticed(
            c,
            to,
            `resume:${message.id}`,
            "Went back to an earlier session.",
        );
    });
    return { type: "back", number: Number(message.number) };
}

/**
 * A line for people alone (C30): the model never reads it, and pi-durable
 * appends it at once, even mid-run. `requestId` admits it once.
 */
export function notice(requestId: string, text: string): SubmissionDraft {
    return { type: "write", requestId, entry: noticeEntry(text) };
}

function noticed(
    c: AgentCall,
    conversation: ConversationId,
    requestId: string,
    text: string,
): Promise<void> {
    return submittedTo(c, conversation, notice(requestId, text));
}

async function submittedTo(
    c: AgentCall,
    conversation: ConversationId,
    draft: SubmissionDraft,
): Promise<void> {
    await withHarness(c, generationOf(c).options, async (harness, context) => {
        const kept = await harness.conversation(conversation, context);
        if (kept === undefined)
            throw new Error("the session has no conversation");
        await kept.submit(draft, context);
    });
}

async function lapsed(
    c: AgentCall,
    conversation: ConversationId,
    why: LapseReason,
): Promise<void> {
    await generationOf(c).peers.reach.lapse({
        conversation: refOf(conversation),
        why,
    });
}

/**
 * Carries on the current session's failed or stopped turn as a visible new
 * input with a task of its own (H12). The earlier answer stays. As today, a
 * session with nothing in it is carried on too.
 */
export async function retry(
    c: AgentCall,
    message: Retry,
): Promise<RetryAnswer> {
    const generation = generationOf(c);
    const answered = generation.inTurn(() => retried(c, message));
    // Set while the retry is still queued, so a stop sent after it waits
    // for it and for every retry queued before it.
    const settled = answered.then(
        () => undefined,
        () => undefined,
    );
    generation.retrying = settled;
    void settled.then(() => {
        if (generation.retrying === settled) generation.retrying = undefined;
    });
    return answered;
}

async function retried(c: AgentCall, message: Retry): Promise<RetryAnswer> {
    const requestId = `retry:${message.id}`;
    const { options, peers } = generationOf(c);
    const conversation = await chiefSession(c);
    const why = await withHarness(c, options, (harness, context) =>
        refusedRetry(harness, conversation, requestId, context),
    );
    if (why === "admitted") return { type: "done" };
    if (why !== undefined) return retryRefused(why);
    // The client's request, so a repeat after a kill opens no second task.
    const opened = await peers.reach.open_task({
        request: message.id,
        turn: "retry",
    });
    if (opened.type === "refused") throw refused(opened.words);
    await submittedTo(
        c,
        conversation,
        draftOf({
            kind: "retry",
            id: requestId,
            // As `crates/hamlet_server/src/talk/chats.rs` words it.
            content: "Carry on from where you stopped.",
            task: opened.task,
        }),
    );
    return { type: "done" };
}

type RetryRefusal = "running" | "answered";

const RETRY_WORDS: Readonly<Record<RetryRefusal, string>> = {
    // As `crates/hamlet_server/src/society/refusal.rs` words them.
    running: "a turn is running here; stop it, then retry",
    answered: "the last message was answered, so there is nothing to carry on",
};

function retryRefused(why: RetryRefusal): RetryAnswer {
    return { type: "refused", why: { type: why }, words: RETRY_WORDS[why] };
}

async function refusedRetry(
    harness: Harness,
    conversation: ConversationId,
    requestId: string,
    context: Context,
): Promise<RetryRefusal | "admitted" | undefined> {
    const admitted = await harness.commit(
        (tx) => tx.submissionByRequest(conversation, requestId),
        context,
    );
    if (admitted !== undefined) return "admitted";
    const live = await harness.snapshot(LiveDoc, conversation, context);
    if (live?.run !== undefined) return "running";
    if (live?.runFailure !== undefined) return undefined;
    // As today, only a session whose last entry is an answer was answered;
    // an answer with nothing in it failed its turn (C29).
    const kept = await harness.conversation(conversation, context);
    const [last] =
        (await kept?.entries({}, 1, undefined, context))?.items ?? [];
    const answer =
        last?.kind === "pi.assistant"
            ? (last.model?.[0] as AssistantMessage | undefined)
            : undefined;
    return answer !== undefined && stands(answer) ? "answered" : undefined;
}

/**
 * Takes the current session back to before the chief's line `item`: the
 * session points at a fork made at the entry before it. The workspace and
 * the realm are told first, so the repeat of a rewind cut off before the
 * move tells them again; one after the move finds no such line.
 */
export async function rewind(
    c: AgentCall,
    message: Rewind,
): Promise<RewindAnswer> {
    if (message.chat.from !== undefined || message.chat.past !== undefined) {
        return {
            type: "refused",
            why: { type: "not_current" },
            // As `crates/hamlet_server/src/clients/requests.rs` words it.
            words: "Only the current session of a chief's chat goes back.",
        };
    }
    const at = entryOf(message.item);
    const { options, inTurn, peers } = generationOf(c);
    return inTurn(async () => {
        const from = await current(c.db);
        if (from === undefined || at === undefined) {
            return rewindRefused("no_such_item", message.item);
        }
        const found = await withHarness(c, options, (harness, context) =>
            lineAt(harness, from, at, context),
        );
        if (typeof found === "string") {
            return rewindRefused(found, message.item);
        }
        await peers.cancel({
            which: { type: "conversation", conversation: refOf(from) },
        });
        await lapsed(c, from, "rewound");
        const fork = await withHarness(c, options, async (harness, context) => {
            // A timer left set would fire a turn in the session left.
            await timersEnded(harness, from, context);
            const kept = await harness.conversation(from, context);
            const before = found.before;
            const made =
                before === undefined || kept === undefined
                    ? await created(c, harness, context)
                    : await created(c, harness, context, (forked) =>
                          kept.fork(before, forked, context),
                      );
            return made.id;
        });
        await moveTo(c, from, fork);
        return found.rewound;
    });
}

type RewindRefusal = "no_such_item" | "not_a_chief_line" | "working";

const REWIND_WORDS: Readonly<Record<RewindRefusal, string>> = {
    // As `crates/hamlet_server/src/talk/chats.rs` and
    // `crates/hamlet_server/src/society/refusal.rs` word them.
    no_such_item: "That is not one of your lines in this conversation.",
    not_a_chief_line: "That is not one of your lines in this conversation.",
    working: "a turn is running here; stop it, then rewind",
};

function rewindRefused(why: RewindRefusal, item: string): RewindAnswer {
    return {
        type: "refused",
        why: why === "no_such_item" ? { type: why, item } : { type: why },
        words: REWIND_WORDS[why],
    };
}

/** A client names an entry's item `<entry>:<block>`; a line has one block. */
function entryOf(item: string): EntryId | undefined {
    const entry = /^([1-9][0-9]*)(?::0)?$/.exec(item)?.[1];
    return entry === undefined ? undefined : (Number(entry) as EntryId);
}

async function lineAt(
    harness: Harness,
    conversation: ConversationId,
    at: EntryId,
    context: Context,
): Promise<
    | RewindRefusal
    | {
          rewound: Extract<RewindAnswer, { type: "rewound" }>;
          before: EntryId | undefined;
      }
> {
    if (
        (await isBusy(harness, conversation, context)) ||
        (await hasUnendedCall(harness, conversation, context))
    ) {
        return "working";
    }
    const kept = await harness.conversation(conversation, context);
    const page = await kept?.entries({ maxEntryId: at }, 2, undefined, context);
    const [line, before] = page?.items ?? [];
    if (line?.id !== at) return "no_such_item";
    if (line.kind !== "hamlet.line") return "not_a_chief_line";
    const { images, files } = (line.data ?? {}) as Kept;
    return {
        rewound: {
            type: "rewound",
            text: textOf(line),
            ...(images === undefined ? {} : { images }),
            ...(files === undefined ? {} : { files }),
        },
        before: before?.id,
    };
}

async function hasUnendedCall(
    harness: Harness,
    conversation: ConversationId,
    context: Context,
): Promise<boolean> {
    const kept = await harness.conversation(conversation, context);
    let cursor: Cursor | undefined;
    do {
        const page = await kept?.entries({}, 100, cursor, context);
        for (const entry of page?.items ?? []) {
            const result = entry.model?.[0];
            if (
                entry.byTaskId === undefined ||
                result?.role !== "toolResult" ||
                !endsLater(result.details)
            ) {
                continue;
            }
            const call = callOf({ taskId: entry.byTaskId });
            const ended = await harness.commit(
                (tx) => tx.submissionByRequest(entry.conversationId, call),
                context,
            );
            if (ended === undefined) return true;
        }
        cursor = page?.next;
    } while (cursor !== undefined);
    return false;
}

/** As `crates/hamlet_server/src/commands/oversight.rs:249-266` asks. */
export async function reflectNow(c: AgentCall): Answer<"reflect"> {
    return reflectIn(c, await chiefSession(c));
}
