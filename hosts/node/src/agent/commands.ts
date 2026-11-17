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
    LiveDoc,
    type SubmissionDraft,
} from "@earendil-works/pi-durable";
import { withHarness } from "../pi/runtime.ts";
import { type AgentCall, generationOf } from "./actor.ts";
import { hasWordsOrCall } from "./compaction.ts";
import {
    type ClientActions,
    type LapseReason,
    type NewSession,
    type Resume,
    type ResumeAnswer,
    type Retry,
    type RetryAnswer,
    type Rewind,
    type RewindAnswer,
    u32,
} from "./messages.ts";
import { reflectIn, reflectOnLeaving } from "./reflect.ts";
import {
    begin,
    chiefSession,
    created,
    current,
    makeCurrent,
    moveTo,
    nowSeconds,
    session,
} from "./sessions.ts";
import { textOf } from "./transcript.ts";
import { conversationsOf, draftOf, isBusy, refOf } from "./turn.ts";

type Answer<N extends keyof ClientActions> = Promise<
    ReturnType<ClientActions[N]>
>;

/**
 * Stops every run of the agent and drops what each had queued, then, as
 * today, cancels every call the agent has out and lapses all it was
 * trusted with. Both go out even when nothing ran, so the repeat of a stop
 * cut off before them sends them.
 */
export async function stop(c: AgentCall): Answer<"stop"> {
    const generation = generationOf(c);
    const { options, peers } = generation;
    generation.stops += 1;
    const stopped = await withHarness(c, options, async (harness, context) => {
        const each = await Promise.all(
            (await conversationsOf(harness, context)).map(async (id) => {
                if (!(await isBusy(harness, id, context))) return [];
                await (await harness.conversation(id, context))?.abort(context);
                return [id];
            }),
        );
        return each.flat();
    });
    await peers.cancel({ which: { type: "all" } });
    await peers.reach.lapse({ why: "stopped" });
    return { turns: stopped.length };
}

export async function newSession(
    c: AgentCall,
    message: NewSession,
): Answer<"new_session"> {
    const { number, left } = await generationOf(c).inTurn(async () => {
        const begun = await begin(c, message.id);
        // As `crates/hamlet_server/src/talk/chats.rs` writes it.
        await noticed(
            c,
            begun.to,
            `new:${message.id}`,
            "Started a new session.",
        );
        return begun;
    });
    // A repeat ends the session left again: a kill or a failure may have
    // cut the first off after the session began.
    if (left !== undefined) {
        await lapsed(c, left, "ended");
        await reflectOnLeaving(c, left);
    }
    return { number };
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
            await reflectOnLeaving(c, left);
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
    return {
        type: "write",
        requestId,
        entry: {
            kind: "hamlet.notice",
            data: { text, at: nowSeconds() },
        },
    };
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
    const requestId = `retry:${message.id}`;
    const generation = generationOf(c);
    const { options, peers, inTurn } = generation;
    return inTurn(async () => {
        const conversation = await chiefSession(c);
        const why = await withHarness(c, options, (harness, context) =>
            refusedRetry(harness, conversation, requestId, context),
        );
        if (why === "admitted") return { type: "done" };
        if (why !== undefined) return retryRefused(why);
        const stops = generation.stops;
        const opened = await peers.reach.open_task({ turn: "retry" });
        if (opened.type === "refused") throw new Error(opened.words);
        // A stop that came while the task opened stops the retry too.
        if (generation.stops !== stops) return { type: "done" };
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
    });
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
    const kept = await harness.conversation(conversation, context);
    let cursor: Cursor | undefined;
    do {
        const page = await kept?.entries({}, 100, cursor, context);
        const answer = page?.items.find(
            (entry) => entry.kind === "pi.assistant",
        );
        if (answer !== undefined) {
            const message = answer.model?.[0] as AssistantMessage | undefined;
            // An answer with nothing in it failed its turn (C29).
            return message !== undefined &&
                (message.stopReason === "stop" ||
                    message.stopReason === "length") &&
                hasWordsOrCall(message)
                ? "answered"
                : undefined;
        }
        cursor = page?.next;
    } while (cursor !== undefined);
    return undefined;
}

/**
 * Takes the current session back to before the chief's line `item`: the
 * session points at a fork made at the entry before it. The realm is told
 * first, so the repeat of a rewind cut off before the move tells it again;
 * one after the move finds no such line.
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
    const { options, inTurn } = generationOf(c);
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
        await lapsed(c, from, "rewound");
        const fork = await withHarness(c, options, async (harness, context) => {
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
        return { type: "rewound", text: found.text };
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
): Promise<RewindRefusal | { text: string; before: EntryId | undefined }> {
    if (await isBusy(harness, conversation, context)) return "working";
    const kept = await harness.conversation(conversation, context);
    const page = await kept?.entries({ maxEntryId: at }, 2, undefined, context);
    const [line, before] = page?.items ?? [];
    if (line?.id !== at) return "no_such_item";
    if (line.kind !== "hamlet.line") return "not_a_chief_line";
    return { text: textOf(line), before: before?.id };
}

/** As `crates/hamlet_server/src/commands/oversight.rs:249-266` asks. */
export async function reflectNow(c: AgentCall): Answer<"reflect"> {
    return reflectIn(c, await chiefSession(c));
}
