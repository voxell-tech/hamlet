/**
 * The chief's commands to an agent: what each changes and how it is
 * refused. `chat.ts` makes each a client action.
 */
import type {
    ConversationId,
    SubmissionDraft,
} from "@earendil-works/pi-durable";
import { withHarness } from "../pi/runtime.ts";
import { type AgentCall, generationOf } from "./actor.ts";
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
    type Stop,
    u32,
} from "./messages.ts";
import { reflectOnLeaving } from "./reflect.ts";
import { begin, nowSeconds, resumed } from "./sessions.ts";
import { refOf } from "./turn.ts";

type Answer<N extends keyof ClientActions> = Promise<
    ReturnType<ClientActions[N]>
>;

export function stop(c: AgentCall, message: Stop): Answer<"stop"> {
    void c;
    void message;
    return Promise.reject(new Error("not built: U4"));
}

export async function newSession(
    c: AgentCall,
    message: NewSession,
): Answer<"new_session"> {
    const { number, to, left } = await begin(c, message.id);
    // As `crates/hamlet_server/src/talk/chats.rs` writes it.
    await noticed(c, to, `new:${message.id}`, "Started a new session.");
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
    const back = await resumed(c, message.number);
    if (back === undefined) {
        const number = u32(message.number);
        return {
            type: "refused",
            why: { type: "no_such_session", number },
            // As `crates/hamlet_server/src/society/refusal.rs` words it.
            words: `there is no past session ${Number.isSafeInteger(message.number) ? message.number : number} here`,
        };
    }
    await noticed(
        c,
        back.to,
        `resume:${message.id}`,
        "Went back to an earlier session.",
    );
    // A session gone back to asks again what it was trusted with (H11).
    await lapsed(c, back.to, "resumed");
    if (back.left !== undefined) await reflectOnLeaving(c, back.left);
    return { type: "back", number: message.number };
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

async function noticed(
    c: AgentCall,
    conversation: ConversationId,
    requestId: string,
    text: string,
): Promise<void> {
    await withHarness(c, generationOf(c).options, async (harness, context) => {
        const kept = await harness.conversation(conversation, context);
        if (kept === undefined)
            throw new Error("the session has no conversation");
        await kept.submit(notice(requestId, text), context);
    });
}

/** Reported, not thrown: the session has already moved. */
async function lapsed(
    c: AgentCall,
    conversation: ConversationId,
    why: LapseReason,
): Promise<void> {
    const { peers } = generationOf(c);
    try {
        await peers.reach.lapse({
            conversation: refOf(conversation),
            why,
        });
    } catch (error) {
        peers.report(`the realm was not told the session ${why}`, error);
    }
}

export function retry(c: AgentCall, message: Retry): Promise<RetryAnswer> {
    void c;
    void message;
    return Promise.reject(new Error("not built: U4"));
}

export function rewind(c: AgentCall, message: Rewind): Promise<RewindAnswer> {
    void c;
    void message;
    return Promise.reject(new Error("not built: U4"));
}

export function reflectNow(c: AgentCall): Answer<"reflect"> {
    void c;
    return Promise.reject(new Error("not built: U5"));
}
