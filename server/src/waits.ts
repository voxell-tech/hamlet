import type { Context } from "@earendil-works/chord";
import {
    type ConversationId,
    defineDoc,
    defineDocFamily,
    type Harness,
    type TaskId,
    type Tx,
} from "@earendil-works/pi-durable";
import type { ChatId, With } from "./chats.ts";
import type { AgentRef } from "./hamlet.ts";
import { localDay, untilAborted } from "./home.ts";

export type WaitKind = "approval" | "fault" | "question" | "objection";

/** The reason a call needs a chief's approval (H11). */
export type Danger =
    | "network"
    | "unconfined"
    | "destructive"
    | "reserved"
    | "edit"
    | "command";

/** A chief's answer to a wait. */
export type Choice =
    | "allow"
    | "always"
    | "deny"
    | "go"
    | "withdraw"
    | "reply"
    | "dismiss";

/** The end of a wait: a chief's answer, or an end that came without one (H34). */
export type How = Choice | "stopped" | "rewound" | "lapsed";

/** The change an edit asks for, at most 200 lines of 240 characters (C30). */
export type Change = {
    /** The file, as the call named it. */
    path: string;
    /** True when the call makes the file. */
    created: boolean;
    lines: { mark: " " | "+" | "-"; text: string }[];
    /** The lines of the whole change, those that stay among them. */
    total: number;
    added: number;
    removed: number;
    /** The count of its lines, in words. */
    count: string;
};

/** A tool call a wait holds. */
export type HeldCall = {
    task: TaskId;
    conversation: ConversationId;
};

/** Something that waits for a chief (H11, H34). */
export type Wait = {
    id: string;
    kind: WaitKind;
    danger: Danger | null;
    /** The name of the agent that raised it. */
    by: string;
    agent: AgentRef | null;
    /** The exact text the chief reads. */
    about: string;
    change: Change | null;
    answers: Choice[];
    since: number;
    /** Times it was asked while it waited. */
    asked: number;
    /** The chief's chat it was raised in. */
    chat: With | null;
    calls: HeldCall[];
};

/** The waits open for a chief now, by wait id. */
export const OversightDoc = defineDoc<{
    /** Waits raised so far, which numbers the next one. */
    raised: number;
    waits: { [id: string]: Wait };
}>({
    kind: "hamlet.oversight",
    version: 1,
    scope: "session",
    initial: () => ({ raised: 0, waits: {} }),
});

/** A line of the record of what waited and how it ended. */
export type WaitEvent =
    | {
          type: "raised";
          at: number;
          id: string;
          kind: WaitKind;
          by: string;
          agent: AgentRef | null;
          about: string;
      }
    | {
          type: "settled";
          at: number;
          id: string;
          how: How;
          text: string;
          chief: string | null;
      };

/** The record of what waited and how it ended, by local day `YYYY-MM-DD` (H34). */
export const OversightDayDoc = defineDocFamily<{ events: WaitEvent[] }, string>(
    {
        kind: "hamlet.oversight-day",
        version: 1,
        scope: "session",
        family: true,
        initial: () => ({ events: [] }),
    },
);

/** The end of the wait that held a call, `how` null while it waits. */
export type Ruling = {
    how: How | null;
    text: string;
};

/** The ruling on one held call, kept by the call's tool task. */
export const RulingDoc = defineDoc<Ruling>({
    kind: "hamlet.ruling",
    version: 1,
    scope: "task",
    initial: () => ({ how: null, text: "" }),
});

/** A wait to raise, and the call it holds. */
export type Raising = Omit<Wait, "id" | "since" | "asked" | "calls"> & {
    call: HeldCall | null;
};

/** The end given to a wait, and the chief who gave it. */
export type Ending = {
    how: How;
    text: string;
    chief: string | null;
};

/**
 * Raises a wait, or joins the one that waits for the same thing by the same agent. A call already held finds its wait
 * again; one already ruled raises nothing, and gives `undefined`.
 */
export async function raise(
    tx: Tx,
    raising: Raising,
    at: number,
): Promise<string | undefined> {
    const { call } = raising;
    if (call !== null) {
        if ((await tx.doc(RulingDoc, call.task)).how !== null) return undefined;
        const waits = (await tx.doc(OversightDoc)).waits;
        const holding = Object.values(waits).find((wait) =>
            wait.calls.some((held) => held.task === call.task),
        );
        if (holding !== undefined) return holding.id;
    }
    return put(tx, raising, at);
}

/** Raises a fault: a turn that failed, which a chief can only dismiss. The same fault again is asked again. */
export function raiseFault(
    tx: Tx,
    fault: Pick<Wait, "by" | "agent" | "about" | "chat">,
    at: number,
): Promise<string> {
    return put(
        tx,
        {
            ...fault,
            kind: "fault",
            danger: null,
            change: null,
            answers: ["dismiss"],
            call: null,
        },
        at,
    );
}

/**
 * Ends wait `id`, rules on each call it held and records how it ended. Gives the wait, or `undefined` when nothing
 * waits as `id`. It reads tasks, so it comes before the commit's first table write.
 */
export async function settle(
    tx: Tx,
    id: string,
    ending: Ending,
    at: number,
): Promise<Wait | undefined> {
    const oversight = await tx.doc(OversightDoc);
    const wait = oversight.waits[id];
    if (wait === undefined) return undefined;
    const settled = JSON.parse(JSON.stringify(wait)) as Wait;
    for (const { task } of settled.calls) {
        const status = (await tx.task(task))?.state.status;
        // A task that ended, such as one stopped, has nothing to rule on.
        if (
            status === undefined ||
            status === "terminal" ||
            status === "completing"
        ) {
            continue;
        }
        Object.assign(await tx.doc(RulingDoc, task), {
            how: ending.how,
            text: ending.text,
        });
    }
    delete oversight.waits[id];
    await note(tx, { type: "settled", at, id, ...ending });
    return settled;
}

/** Dismisses wait `id` for `chief`. */
export function dismiss(
    tx: Tx,
    id: string,
    chief: string,
    at: number,
): Promise<Wait | undefined> {
    return settle(tx, id, { how: "dismiss", text: "", chief }, at);
}

/**
 * Ends the waits raised in `chat`, as `stopped` (faults stay) or as `rewound` from `since` on. It reads tasks, so it
 * comes before the commit's first table write.
 */
export async function lapse(
    tx: Tx,
    chat: ChatId,
    ending: {
        how: "stopped" | "rewound";
        chief: string | null;
        since?: number;
        /** The numbered conversation whose waits lapse, when supplied. */
        conversation?: number;
    },
    at: number,
): Promise<void> {
    const { how, chief, since = 0 } = ending;
    const ids = Object.values((await tx.doc(OversightDoc)).waits)
        .filter(
            (wait) =>
                wait.chat !== null &&
                wait.chat.hamlet === chat.agent.hamlet &&
                wait.chat.number === chat.agent.number &&
                wait.chat.chief === chat.chief &&
                (ending.conversation === undefined ||
                    wait.chat.chat === ending.conversation) &&
                wait.since >= since &&
                !(how === "stopped" && wait.kind === "fault"),
        )
        .map((wait) => wait.id);
    for (const id of ids) await settle(tx, id, { how, text: "", chief }, at);
}

/** The ruling on the call of tool task `task`, once a wait raised for it has ended. */
export async function ruling(
    harness: Harness,
    task: TaskId,
    context: Context,
): Promise<Ruling & { how: How }> {
    const state = await harness.documentState(RulingDoc, task, context);
    if (state === undefined) throw new Error(`No wait was raised for ${task}`);
    try {
        return await Promise.race([
            new Promise<Ruling & { how: How }>((resolve) => {
                const check = (value: Readonly<Ruling> | null) => {
                    if (value !== null && value.how !== null) {
                        resolve({ how: value.how, text: value.text });
                    }
                };
                check(state.value);
                state.subscribe(check);
            }),
            untilAborted(context),
        ]);
    } finally {
        state.dispose();
    }
}

/** Opens a wait, or asks again the one that waits for the same thing by the same agent. */
async function put(tx: Tx, raising: Raising, at: number): Promise<string> {
    const { call, ...wait } = raising;
    const oversight = await tx.doc(OversightDoc);
    const same = Object.values(oversight.waits).find(
        (open) =>
            open.kind === wait.kind &&
            open.danger === wait.danger &&
            open.by === wait.by &&
            open.about === wait.about,
    );
    if (same !== undefined) {
        same.asked += 1;
        if (call !== null) same.calls.push(call);
        return same.id;
    }
    oversight.raised += 1;
    const id = `${at}-${oversight.raised}`;
    oversight.waits[id] = {
        ...wait,
        id,
        since: at,
        asked: 1,
        calls: call === null ? [] : [call],
    };
    await note(tx, {
        type: "raised",
        at,
        id,
        kind: wait.kind,
        by: wait.by,
        agent: wait.agent,
        about: wait.about,
    });
    return id;
}

/** Appends `event` to the record of its day. */
async function note(tx: Tx, event: WaitEvent): Promise<void> {
    const day = localDay(event.at);
    (await tx.doc(OversightDayDoc, day, day)).events.push(event);
}

/** The lapse of every wait raised by a departing agent (H28). */
export async function lapseAgent(
    tx: Tx,
    agent: AgentRef,
    at: number,
): Promise<void> {
    const waits = Object.values((await tx.doc(OversightDoc)).waits).filter(
        (w) =>
            w.agent?.hamlet === agent.hamlet && w.agent.number === agent.number,
    );
    for (const wait of waits)
        await settle(tx, wait.id, { how: "lapsed", text: "", chief: null }, at);
}
