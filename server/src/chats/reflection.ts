import type { Context } from "@earendil-works/chord";
import {
    type ConversationId,
    defineDoc,
    type Submission,
    type SubmissionId,
    type Tx,
} from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import { agentKey, RecordDoc } from "../record.ts";
import { SettingsDoc } from "../settings.ts";
import {
    ChatDoc,
    ReflectionEntry,
    reflectionInput,
    WithDoc,
} from "./conversations.ts";
import { notify } from "./notice.ts";
import { newTask, type Turn } from "./turn.ts";

/** Successfully answered chief turns since the last reflection (C5). */
export const ReflectionDoc = defineDoc<{ unreflected: number }>({
    kind: "hamlet.reflection-count",
    version: 1,
    scope: "conversation",
    history: "rewindable",
    fork: "asOf",
    initial: () => ({ unreflected: 0 }),
});

type Reservation = {
    ordinal: number;
    count: number;
    task: string;
    at: number;
    requestId: string;
};
/** Reserved reflections survive a crash before their deduplicated submissions. */
export const ReflectDoc = defineDoc<{
    pending: Reservation[];
    entered: SubmissionId | null;
    retiring: boolean;
}>({
    kind: "hamlet.reflection-job",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "initial",
    initial: () => ({ pending: [], entered: null, retiring: false }),
});

/** The automatic instruction to save session lessons in notes and skills. */
export const REFLECT_PROMPT =
    "Reflection time (automatic, not from a user). Look back over this session. Update NOTES.md with anything worth remembering about the users or lessons learned, and save or improve a skill for anything you did that is likely to come up again. If nothing is worth saving, say so in one line.";

/** The environment override is installation-wide, including reflections at close. */
export async function reflectEvery(
    home: Home,
    context: Context,
): Promise<number> {
    return (
        home.overrides.reflectEvery ??
        (await home.harness.snapshot(SettingsDoc, context))?.reflectEvery ??
        10
    );
}

/** Marks a reflection submission once and resets its answered-turn count. */
export async function enterReflection(
    tx: Tx,
    conversation: ConversationId,
    turn: Turn,
): Promise<void> {
    if (turn.kind !== ReflectionEntry.kind) return;
    const job = await tx.doc(ReflectDoc, conversation);
    if (job.entered === turn.input) return;
    job.entered = turn.input;
    (await tx.doc(ReflectionDoc, conversation)).unreflected = 0;
}

/** Reserves ordinals before submitting under those identities; either half can be resumed (S3). */
export async function requestReflection(
    home: Home,
    id: ConversationId,
    context: Context,
    mode: "automatic" | "owed" | "explicit" = "automatic",
): Promise<Submission | undefined> {
    const every = await reflectEvery(home, context);
    const reservations = await home.harness.commit(async (tx) => {
        const place = await tx.doc(WithDoc, id);
        if (place.chief === null) return undefined;
        const key = agentKey(place);
        const chat = (await tx.doc(ChatDoc, key, key)).chats[place.chief];
        if (!chat?.conversations.some((each) => each.conversation === id))
            return undefined;
        const job = await tx.doc(ReflectDoc, id);
        if (job.pending.length > 0 && mode !== "explicit")
            return job.pending.map((each) => ({ ...each }));
        const count = (await tx.doc(ReflectionDoc, id)).unreflected;
        if (
            (count === 0 && mode !== "explicit") ||
            (mode === "automatic" &&
                (every === 0 || (!job.retiring && count < every)))
        )
            return undefined;
        const ordinal = ++chat.reflections;
        job.pending.push({
            ordinal,
            count,
            task: newTask(home.now()),
            at: home.now(),
            requestId: `reflect:${place.chat}:${ordinal}`,
        });
        if (job.retiring)
            await notify(
                tx,
                id,
                "Putting this session away. The agent keeps what is worth keeping first.",
                home.now(),
            );
        await notify(
            tx,
            id,
            `Reflecting ${count} turn${count === 1 ? "" : "s"}.`,
            home.now(),
        );
        return job.pending.map((each) => ({ ...each }));
    }, context);
    if (reservations === undefined) return undefined;
    const conversation = await home.harness.conversation(id, context);
    const place = await home.harness.snapshot(WithDoc, id, context);
    const record =
        place === undefined
            ? undefined
            : await home.harness.snapshot(RecordDoc, place.hamlet, context);
    const words =
        REFLECT_PROMPT +
        (place !== undefined && place.number !== record?.mayor
            ? "\nThen, in one line: did anything about how the hamlet is run get in the way of your work? If nothing did, say so."
            : "");
    let submitted: Submission | undefined;
    for (const job of reservations)
        submitted = await conversation?.submit(
            {
                ...reflectionInput(
                    place?.chat ?? 0,
                    job.ordinal,
                    words,
                    job.task,
                    job.at,
                ),
                requestId: job.requestId,
            },
            context,
        );
    return submitted;
}
