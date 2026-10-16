import { type Context, copyJson } from "@earendil-works/chord";
import {
    type ConversationId,
    type DocumentReader,
    defineDoc,
    defineDocFamily,
    type Harness,
    type TaskId,
    type Tx,
} from "@earendil-works/pi-durable";
import type { Static } from "typebox";
import type { ChatId, With } from "../chats/conversations.ts";
import { untilAborted } from "../home/abort.ts";
import { localDay } from "../home/clock.ts";
import type * as protocol from "../protocol.ts";
import {
    type AgentRef,
    agentKey,
    type HamletRecord,
} from "../record/document.ts";
import { addressOf } from "../record/names.ts";
import { says } from "./words.ts";

/** The reason an agent is waiting for a chief. */
export type WaitKind = "approval" | "fault" | "question" | "objection";

/** The reason a call needs a chief's approval (H11). */
export type Danger = Static<typeof protocol.WaitDanger>;

/** A chief's answer to a wait. */
export type Choice = protocol.Choice;

/** The end of a wait, including ends without a chief's answer. */
export type How = Choice | "stopped" | "rewound" | "lapsed";

/** The bounded edit preview read by a chief (C30). */
export type Change = protocol.Change;

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
    // SAFETY: copyJson detaches the typed transaction draft; its return type loses the document shape.
    // Verified by: waits.test.ts settlement across restart and stop.test.ts held-turn settlement.
    const settled = copyJson(wait) as Wait;
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

/** Open waits in raise order, also usable within a commit. */
export async function open(
    read: DocumentReader | Tx,
    context: Context,
): Promise<readonly Wait[]> {
    const oversight =
        "doc" in read
            ? await read.doc(OversightDoc)
            : await read.snapshot(OversightDoc, context);
    const order = (wait: Wait) =>
        Number(wait.id.slice(wait.id.lastIndexOf("-") + 1));
    return Object.values(oversight?.waits ?? {}).sort(
        (a, b) => order(a) - order(b),
    );
}

/** Waits raised by the stable agent identity, across its chats. */
export function forAgent(
    waits: readonly Wait[],
    ref: AgentRef,
): readonly Wait[] {
    return waits.filter(
        (wait) =>
            wait.agent?.hamlet === ref.hamlet &&
            wait.agent.number === ref.number,
    );
}

/** Waits belonging to a chief's chat, across its numbered conversations. */
export function forChat(waits: readonly Wait[], chat: ChatId): readonly Wait[] {
    return waits.filter(
        (wait) =>
            wait.chat?.hamlet === chat.agent.hamlet &&
            wait.chat.number === chat.agent.number &&
            wait.chat.chief === chat.chief,
    );
}

/** Recorded endings through `at`, using local calendar days rather than 24-hour steps. */
export async function ended(
    read: DocumentReader,
    ids: readonly string[],
    context: Context,
    at = Date.now(),
): Promise<Map<string, How>> {
    const wanted = new Set(ids);
    const times = ids
        .map((id) => Number(id.slice(0, id.lastIndexOf("-"))))
        .filter(Number.isFinite);
    const found = new Map<string, How>();
    if (times.length === 0) return found;
    const day = new Date(Math.min(at, ...times));
    const last = localDay(Math.max(at, ...times));
    while (localDay(day.getTime()) <= last) {
        for (const event of (
            await read.snapshot(
                OversightDayDoc,
                localDay(day.getTime()),
                context,
            )
        )?.events ?? []) {
            if (event.type === "settled" && wanted.has(event.id))
                found.set(event.id, event.how);
        }
        day.setDate(day.getDate() + 1);
    }
    return found;
}

/** A chief's view; current chat numbers omit the past-session selector. */
export function view(
    wait: Wait,
    records: Readonly<Record<string, Readonly<HamletRecord>>>,
    currentChats: ReadonlyMap<
        string,
        Readonly<Record<string, number>>
    > = new Map(),
): protocol.Wait {
    const record = wait.chat && records[wait.chat.hamlet];
    return {
        id: wait.id,
        ...(wait.kind === "approval"
            ? { kind: "approval" as const, danger: wait.danger ?? "unconfined" }
            : { kind: wait.kind, danger: null }),
        by: wait.by,
        agent: wait.agent && { ...wait.agent },
        says: says(wait.danger),
        about: wait.about,
        change: wait.change && structuredClone(wait.change),
        answers: [...wait.answers],
        since: wait.since,
        asked: wait.asked,
        chat:
            wait.chat && record
                ? {
                      with: {
                          hamlet: wait.chat.hamlet,
                          agent: addressOf(record, wait.chat.number),
                      },
                      ...(currentChats.get(agentKey(wait.chat))?.[
                          wait.chat.chief ?? ""
                      ] === wait.chat.chat
                          ? {}
                          : { past: wait.chat.chat }),
                  }
                : null,
    };
}
