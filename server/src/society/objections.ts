import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import type { Context } from "@earendil-works/chord";
import {
    BACKGROUND_CONTEXT,
    withAbortSignal,
} from "@earendil-works/chord/context";
import { Type } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    defineDoc,
    defineDocFamily,
    defineExtension,
    defineTool,
    GenerationTask,
    type HookApi,
    hook,
    LiveDoc,
    type SubmissionId,
    type ToolExecutionApi,
    ToolTask,
    type Tx,
} from "@earendil-works/pi-durable";
import { AskedDoc } from "../asked.ts";
import { ErrandEntry, RetryEntry, WithDoc } from "../chats.ts";
import { choose } from "../choice.ts";
import { runErrand, startErrand } from "../errands.ts";
import type { Home, Part } from "../home.ts";
import { houseOf, localDay, untilAborted } from "../home.ts";
import { InstructionsDoc } from "../instructions.ts";
import { booksOf } from "../ledger.ts";
import { LimitsDoc, limitsOf } from "../limits.ts";
import { notify } from "../notice.ts";
import { liveHamlets, RealmDoc } from "../realm.ts";
import { type AgentRef, addressOf, agentKey, RecordDoc } from "../record.ts";
import { stopped } from "../report.ts";
import { closedPolicy, SandboxEnv } from "../sandbox.ts";
import { modelRef, SettingsDoc, settingsOf } from "../settings.ts";
import { TaskDoc } from "../tasks.ts";
import { turnOf } from "../turn.ts";
import {
    OversightDayDoc,
    OversightDoc,
    type Ruling,
    RulingDoc,
    raise,
    ruling,
    settle,
} from "../waits.ts";
import { AgentConversationsDoc } from "./docs.ts";

const ToolWaitDoc = defineDoc<{
    wait: string | null;
    result: string | null;
    ruled: Ruling | null;
}>({
    kind: "hamlet.objection-call",
    version: 1,
    scope: "task",
    initial: () => ({ wait: null, result: null, ruled: null }),
});
const InstructionDoc = defineDocFamily<
    {
        withdrawn: string | null;
        notified: boolean;
        waits: string[];
    },
    string
>({
    kind: "hamlet.instruction-ruling",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ withdrawn: null, notified: false, waits: [] }),
});

type Review = {
    task: string;
    conversation: ConversationId;
    input: SubmissionId;
    agent: AgentRef;
    errand: ConversationId;
    text: string;
    done: boolean;
    wait: { id: string; since: number } | null;
};
const ReviewsDoc = defineDoc<{ jobs: Review[] }>({
    kind: "hamlet.reviews",
    version: 1,
    scope: "session",
    initial: () => ({ jobs: [] }),
});

/** The hearings of agents' objections (H10). */
export const Objections: Part = (home) => {
    const controller = new AbortController();
    const background = withAbortSignal(controller.signal, BACKGROUND_CONTEXT);
    const running = new Map<string, Promise<void>>();
    let unsubscribe: (() => void) | undefined;
    let pending = Promise.resolve();
    const launch = (job: Readonly<Review>) => {
        if (job.done || running.has(job.task)) return;
        const work = (async () => {
            // Recovery must not start scheduling before the remaining parts start.
            while (
                (await home.harness.inspect(background)).scheduling === "paused"
            )
                await delay(10, undefined, { signal: controller.signal });
            await review(home, job, background);
            await expireReviews(home, background);
        })()
            .catch(() => {})
            .finally(() => running.delete(job.task));
        running.set(job.task, work);
    };
    return {
        extension: defineExtension({
            name: "hamlet.objections",
            tools: [
                defineTool({
                    name: "object",
                    description:
                        "Tell the chief why the instruction you were just given is a mistake. The work waits for its reply. You may object once; after go ahead, do the work.",
                    parameters: Type.Object({ reason: Type.String() }),
                    replay: "safe",
                    execute: async (args, api, context) =>
                        output(
                            await object(
                                home,
                                api,
                                args.reason.trim(),
                                context,
                            ),
                        ),
                }),
                defineTool({
                    name: "ask_chief",
                    description:
                        "Ask the chief a question you cannot go on without. Waits for the answer, so ask only when no agent and no file can tell you. Use object to argue against an instruction.",
                    parameters: Type.Object({ question: Type.String() }),
                    replay: "safe",
                    execute: async (args, api, context) =>
                        output(
                            await askChief(
                                home,
                                api,
                                args.question.trim(),
                                context,
                            ),
                        ),
                }),
            ],
            hooks: [
                hook(ToolTask, {
                    beforeTool: async (call, api, context) => {
                        if (call.name !== "object" && call.name !== "ask_chief")
                            return undefined;
                        if (call.name === "object")
                            await api.memo("hamlet.objecting", true, context);
                        // A recovered conversation must pass the approval gate before resuming its own hearing.
                        await home.harness.commit(async (tx) => {
                            const saved = await tx.doc(ToolWaitDoc, api.taskId);
                            const ruled = await tx.doc(RulingDoc, api.taskId);
                            if (saved.wait !== null && ruled.how !== null) {
                                saved.ruled = { ...ruled };
                                Object.assign(
                                    ruled,
                                    RulingDoc.definition.initial(),
                                );
                            }
                        }, context);
                        return undefined;
                    },
                }),
                hook(GenerationTask, {
                    beforeRequest: async (_request, api, context) => {
                        const turn = await turnOf(home.harness, api, context);
                        if (turn?.task)
                            await stopWithdrawn(home, api, turn.task, context);
                        const job = await prepareReview(home, api, context);
                        if (job) launch(job);
                        return undefined;
                    },
                }),
            ],
        }),
        async start(context) {
            for (const job of (await home.harness.snapshot(ReviewsDoc, context))
                ?.jobs ?? [])
                launch(job);
            await expireReviews(home, context);
            unsubscribe = home.harness.subscribeCommits(({ changes }) => {
                if (
                    !changes.some(
                        (c) =>
                            c.type === "submission" ||
                            (c.type === "document" &&
                                (c.record.kind ===
                                    InstructionsDoc.definition.kind ||
                                    c.record.kind ===
                                        OversightDayDoc.definition.kind)),
                    )
                )
                    return;
                pending = pending
                    .then(() => expireReviews(home, background))
                    .catch(() => {});
            });
        },
        async stop() {
            unsubscribe?.();
            controller.abort();
            await Promise.allSettled([...running.values(), pending]);
        },
    };
};

function output(text: string) {
    return { output: [{ type: "text" as const, text }] };
}

async function object(
    home: Home,
    api: ToolExecutionApi,
    reason: string,
    context: Context,
): Promise<string> {
    const saved = await api.snapshot(ToolWaitDoc, api.taskId, context);
    if (saved?.result != null) return saved.result;
    const place = await api.snapshot(WithDoc, api.conversationId, context);
    const turn = await turnOf(home.harness, api, context);
    if (!place || turn?.chief == null || turn.task === null)
        return "Only the agent a chief has just instructed may object.";
    if (!reason) return "Say why in `reason`.";
    const key = agentKey(place);
    const task = turn.task;
    const prepared = await api.commit(async (tx) => {
        const state = await tx.doc(ToolWaitDoc, api.taskId);
        if (state.wait !== null) return undefined;
        const instruction = (
            await tx.doc(InstructionsDoc, key, key)
        ).given.find((i) => i.task === task);
        if (!instruction)
            return "Only the agent a chief has just instructed may object.";
        if (instruction.hearing.cleared)
            return "The chief has said to go ahead; you may not object to this instruction again.";
        if (instruction.hearing.agent)
            return "You have already objected to this instruction once; you may not object again.";
        instruction.hearing.agent = true;
        const addressed = addressOf(
            await tx.doc(RecordDoc, place.hamlet, place.hamlet),
            place.number,
        );
        const name = waitName(addressed, place.hamlet);
        state.wait =
            (await raise(
                tx,
                {
                    kind: "objection",
                    danger: null,
                    by: name,
                    agent: { hamlet: place.hamlet, number: place.number },
                    about: `${instruction.chief} told ${name}: ${instruction.text}\n${name} objects: ${reason}`,
                    answers: ["go", "withdraw"],
                    change: null,
                    chat: place,
                    call: {
                        task: api.taskId,
                        conversation: api.conversationId,
                    },
                },
                home.now(),
            )) ?? null;
        if (state.wait !== null)
            (await tx.doc(InstructionDoc, task, task)).waits.push(state.wait);
        return undefined;
    }, context);
    if (prepared) return prepared;
    const decided =
        saved?.ruled ?? (await ruling(home.harness, api.taskId, context));
    const result =
        decided.how === "go"
            ? `The chief says go ahead: ${decided.text} Do the work; you may not object again.`
            : `The chief withdrew the instruction: ${decided.text}`;
    const active =
        decided.how === "go"
            ? []
            : await taskConversations(home, task, context);
    await api.commit(async (tx) => {
        (await tx.doc(ToolWaitDoc, api.taskId)).result = result;
        const instruction = (
            await tx.doc(InstructionsDoc, key, key)
        ).given.find((i) => i.task === task);
        if (decided.how === "go" && instruction)
            instruction.hearing.cleared = true;
        if (decided.how !== "go") {
            (await tx.doc(InstructionDoc, task, task)).withdrawn = result;
            await stopTaskWaits(home, tx, new Set(active));
        }
        await settleOthers(home, tx, task, decided);
    }, context);
    return result;
}

/** The hold before an instructed agent works. */
export async function holdFor(
    home: Home,
    api: HookApi,
    context: Context,
): Promise<void> {
    const turn = await turnOf(home.harness, api, context);
    if (!turn?.task) return;
    const task = turn.task;
    if (!(await api.memo<boolean>("hamlet.objecting", context))) {
        const state = await api.snapshot(InstructionDoc, task, context);
        for (const id of state?.waits ?? []) {
            const wait = (await api.snapshot(OversightDoc, context))?.waits[id];
            if (!wait?.answers.includes("withdraw")) continue;
            const decided = await reviewRuling(
                home,
                { id, since: wait.since },
                context,
            );
            const active =
                decided.how === "withdraw"
                    ? await taskConversations(home, task, context)
                    : [];
            await home.harness.commit(async (tx) => {
                if (decided.how === "withdraw") {
                    (await tx.doc(InstructionDoc, task, task)).withdrawn =
                        `The chief withdrew the instruction: ${decided.text}`;
                    await stopTaskWaits(home, tx, new Set(active));
                }
                await settleOthers(home, tx, task, decided);
            }, context);
        }
    }
    await stopWithdrawn(home, api, task, context);
}

async function stopWithdrawn(
    home: Home,
    api: HookApi,
    task: string,
    context: Context,
): Promise<void> {
    const state = await api.snapshot(InstructionDoc, task, context);
    if (state?.withdrawn) {
        await abortWithdrawn(home, api.conversationId, task, context);
        return untilAborted(context);
    }
}

async function askChief(
    home: Home,
    api: ToolExecutionApi,
    question: string,
    context: Context,
): Promise<string> {
    const saved = await api.snapshot(ToolWaitDoc, api.taskId, context);
    if (saved?.result != null) return saved.result;
    const place = await api.snapshot(WithDoc, api.conversationId, context);
    const turn = await turnOf(home.harness, api, context);
    const task = turn?.task;
    const chief =
        (task ? (await api.snapshot(TaskDoc, task, context))?.chief : null) ??
        (turn?.kind === RetryEntry.kind ? place?.chief : null);
    if (!place || !turn || !chief)
        return "No chief oversees this turn; go on with what you know.";
    if (!question) return "Say what in `question`.";
    const limits = limitsOf(
        await api.snapshot(LimitsDoc, place.hamlet, context),
    );
    const prepared = await api.commit(async (tx) => {
        const state = await tx.doc(ToolWaitDoc, api.taskId);
        if (state.wait !== null) return undefined;
        const asked = await tx.doc(AskedDoc, api.conversationId);
        if (asked.input !== turn.input)
            Object.assign(asked, AskedDoc.definition.initial(), {
                input: turn.input,
            });
        if (asked.questions >= limits.asking.questions)
            return `You have asked the chief ${limits.asking.questions} questions this turn; go on with what you know.`;
        asked.questions++;
        const addressed = addressOf(
            await tx.doc(RecordDoc, place.hamlet, place.hamlet),
            place.number,
        );
        const name = waitName(addressed, place.hamlet);
        state.wait =
            (await raise(
                tx,
                {
                    kind: "question",
                    danger: null,
                    by: name,
                    agent: { hamlet: place.hamlet, number: place.number },
                    about: question,
                    answers: ["reply", "dismiss"],
                    change: null,
                    chat: place.chief === null ? null : place,
                    call: {
                        task: api.taskId,
                        conversation: api.conversationId,
                    },
                },
                home.now(),
            )) ?? null;
        return undefined;
    }, context);
    if (prepared) return prepared;
    const decided =
        saved?.ruled ?? (await ruling(home.harness, api.taskId, context));
    const result =
        decided.how === "reply"
            ? `The chief answers: ${decided.text}`
            : "The chief did not answer; go on with what you know.";
    await api.commit(async (tx) => {
        (await tx.doc(ToolWaitDoc, api.taskId)).result = result;
    }, context);
    return result;
}

async function prepareReview(
    home: Home,
    api: HookApi,
    context: Context,
): Promise<Readonly<Review> | undefined> {
    const turn = await turnOf(home.harness, api, context);
    if (turn?.chief == null || turn.task === null) return undefined;
    const known = (await api.snapshot(ReviewsDoc, context))?.jobs.find(
        (j) => j.task === turn.task,
    );
    if (known) return known;
    const place = await api.snapshot(WithDoc, api.conversationId, context);
    if (!place) return undefined;
    const record = await api.snapshot(RecordDoc, place.hamlet, context);
    if (!record || record.mayor === place.number) return undefined;
    const limits = limitsOf(
        await api.snapshot(LimitsDoc, place.hamlet, context),
    );
    if (limits.asking.mayorReview === "never") return undefined;
    const mayor = record.agents.find((a) => a.number === record.mayor);
    if (!mayor) return undefined;
    const settings = settingsOf(
        await api.snapshot(SettingsDoc, context),
        home.overrides,
    );
    const model = modelRef(
        choose(limits, settings, {
            name: mayor.name,
            mayor: true,
            model: mayor.model,
        }),
    );
    const url =
        settings.endpoints.find((e) => e.name === model?.provider)?.baseUrl ??
        (model && home.models.getProvider(model.provider)?.baseUrl) ??
        "";
    if (limits.asking.mayorReview === "remote" && !remote(url))
        return undefined;
    if (
        stopped(
            limits,
            await booksOf(
                home,
                place.hamlet,
                limits.budget.period,
                turn.task,
                context,
            ),
        )
    )
        return undefined;
    const key = agentKey(place);
    const instruction = (
        await api.snapshot(InstructionsDoc, key, context)
    )?.given.find((i) => i.task === turn.task);
    if (!instruction) return undefined;
    const agent = addressOf(record, place.number);
    const notes = await reviewNotes(
        home,
        { hamlet: place.hamlet, number: mayor.number },
        context,
    );
    return home.harness.commit(async (tx) => {
        const jobs = (await tx.doc(ReviewsDoc)).jobs;
        const known = jobs.find((j) => j.task === turn.task);
        if (known) return { ...known };
        const errand = await startErrand(home, tx, {
            agent: { hamlet: place.hamlet, number: mayor.number },
            kind: "review",
            own: false,
            tools: [],
            instructions: `You are the mayor of a hamlet of agents. A chief, a human the hamlet belongs to, has just instructed one of your agents directly. The work has already begun and the chief always wins. If you believe the instruction is a mistake for the hamlet, answer OBJECT: <reason>: the work is then held until the chief replies. You may do this once. If it is fine, or you are unsure, reply with the single word pass. Most instructions are fine.\n# Your agents\n${record.agents
                .filter((a) => a.number !== record.mayor)
                .map((a) => `- ${a.name}: ${a.purpose}`)
                .join("\n")}\n# Your notes\n${notes}`,
        });
        const job: Review = {
            task: turn.task ?? "",
            input: turn.input,
            agent: { hamlet: place.hamlet, number: place.number },
            conversation: api.conversationId,
            errand,
            text: `${instruction.chief} told \`${agent}\`: ${instruction.text}`,
            done: false,
            wait: null,
        };
        jobs.push(job);
        return job;
    }, context);
}

async function reviewNotes(
    home: Home,
    agent: AgentRef,
    context: Context,
): Promise<string> {
    const house = houseOf(home.layout, agent);
    const policy = closedPolicy(house, [house]);
    policy.rw = [];
    const env = new SandboxEnv({ cwd: house, policy, sandbox: home.sandbox });
    try {
        const read = await env.readTextFile(join(house, "NOTES.md"), context);
        return read.ok ? [...read.value].slice(0, 6000).join("") : "";
    } finally {
        await env.cleanup(context);
    }
}

function remote(url: string): boolean {
    const host = url.replace(/^.*?:\/\//, "").split("/")[0] ?? "";
    const name = host.startsWith("[")
        ? host.slice(1).split("]")[0]
        : host.split(":")[0];
    return (
        !!name &&
        name !== "localhost" &&
        name !== "::1" &&
        name !== "0.0.0.0" &&
        !name.startsWith("127.")
    );
}

async function review(
    home: Home,
    job: Readonly<Review>,
    context: Context,
): Promise<void> {
    const said = await runErrand(
        home,
        job.errand,
        { requestId: `review:${job.task}`, text: job.text, task: job.task },
        context,
    );
    const reason =
        said.status === "done"
            ? /^OBJECT:\s*(\S[\s\S]*)$/i.exec(said.text.trim())?.[1]?.trim()
            : undefined;
    const status = await (
        await home.harness.submission(job.input, context)
    )?.status(context);
    const ended =
        status?.status !== "queued" &&
        status?.status !== "placed" &&
        (await taskConversations(home, job.task, context)).length === 0;
    await home.harness.commit(async (tx) => {
        const current = (await tx.doc(ReviewsDoc)).jobs.find(
            (j) => j.task === job.task,
        );
        if (!current || current.done) return;
        current.done = true;
        if (!reason) return;
        const key = agentKey(job.agent);
        const told = (await tx.doc(InstructionsDoc, key, key)).given.find(
            (i) => i.task === job.task,
        );
        if (!told || told.hearing.cleared || told.hearing.mayor) return;
        told.hearing.mayor = true;
        const since = home.now();
        const place = await tx.doc(WithDoc, job.conversation);
        const about = `${told.chief} told ${addressOf(await tx.doc(RecordDoc, job.agent.hamlet, job.agent.hamlet), job.agent.number)}: ${told.text}\nThe mayor objects: ${reason}${ended ? "\n(The work had already ended.)" : ""}`;
        const id = await raise(
            tx,
            {
                kind: "objection",
                danger: null,
                by:
                    job.agent.hamlet === "home"
                        ? "mayor"
                        : `mayor@${job.agent.hamlet}`,
                agent: { ...job.agent },
                about,
                answers: ended ? ["dismiss"] : ["go", "withdraw"],
                change: null,
                chat: { ...place },
                call: null,
            },
            since,
        );
        if (id) {
            current.wait = { id, since };
            (await tx.doc(InstructionDoc, job.task, job.task)).waits.push(id);
        }
    }, context);
}

async function reviewRuling(
    home: Home,
    wait: { id: string; since: number },
    context: Context,
): Promise<Ruling> {
    const state = await home.harness.documentState(OversightDoc, context);
    if (!state) return { how: "lapsed", text: "" };
    try {
        await Promise.race([
            new Promise<void>((resolve) => {
                const check = () => {
                    if (!state.value?.waits[wait.id]) resolve();
                };
                check();
                state.subscribe(check);
            }),
            untilAborted(context),
        ]);
        return settledReview(home, wait, context);
    } finally {
        state.dispose();
    }
}

async function settledReview(
    home: Home,
    wait: { id: string; since: number },
    context: Context,
): Promise<Ruling> {
    const day = new Date(home.now());
    const first = localDay(wait.since);
    while (localDay(day.getTime()) >= first) {
        const events =
            (
                await home.harness.snapshot(
                    OversightDayDoc,
                    localDay(day.getTime()),
                    context,
                )
            )?.events ?? [];
        const event = events.findLast(
            (e) => e.type === "settled" && e.id === wait.id,
        );
        if (event?.type === "settled")
            return { how: event.how, text: event.text };
        day.setDate(day.getDate() - 1);
    }
    return { how: "lapsed", text: "" };
}

async function abortWithdrawn(
    home: Home,
    conversation: ConversationId,
    task: string,
    context: Context,
): Promise<void> {
    const active = await taskConversations(home, task, context);
    const conversations = new Set(active);
    await home.harness.commit(async (tx) => {
        const state = await tx.doc(InstructionDoc, task, task);
        if (state.withdrawn === null) return;
        await stopTaskWaits(home, tx, conversations);
        if (!state.notified) {
            state.notified = true;
            await notify(tx, conversation, state.withdrawn, home.now());
        }
    }, context);
    for (const id of conversations) {
        const turn = await turnAt(home, id, context);
        const live = await home.harness.snapshot(LiveDoc, id, context);
        if (turn?.task === task && live?.run?.inputs.includes(turn.input)) {
            // Aborting the captured run cannot stop a later chief's turn in this conversation.
            void home.harness
                .abortTask(live.run.taskId, BACKGROUND_CONTEXT)
                .catch(() => {});
        }
    }
}

async function stopTaskWaits(
    home: Home,
    tx: Tx,
    conversations: ReadonlySet<ConversationId>,
): Promise<void> {
    const waits = Object.values((await tx.doc(OversightDoc)).waits);
    for (const wait of waits) {
        if (
            wait.kind !== "fault" &&
            wait.calls.some((call) => conversations.has(call.conversation))
        )
            await settle(
                tx,
                wait.id,
                {
                    how: "stopped",
                    chief: home.chief,
                    text: "The chief withdrew the instruction.",
                },
                home.now(),
            );
    }
}

async function turnAt(
    home: Home,
    conversationId: ConversationId,
    context: Context,
) {
    return turnOf(
        home.harness,
        {
            snapshot: home.harness.snapshot.bind(home.harness),
            snapshotAsOf: home.harness.snapshotAsOf.bind(home.harness),
            conversationId,
        },
        context,
    );
}

async function taskConversations(
    home: Home,
    task: string,
    context: Context,
): Promise<ConversationId[]> {
    const conversations: ConversationId[] = [];
    const realm = await home.harness.snapshot(RealmDoc, context);
    for (const hamlet of liveHamlets(realm)) {
        const record = await home.harness.snapshot(
            RecordDoc,
            hamlet.id,
            context,
        );
        for (const agent of record?.agents ?? []) {
            const index = await home.harness.snapshot(
                AgentConversationsDoc,
                agentKey({ hamlet: hamlet.id, number: agent.number }),
                context,
            );
            for (const conversationId of index?.ids ?? []) {
                const turn = await turnAt(home, conversationId, context);
                if (turn?.task === task && turn.kind !== ErrandEntry.kind)
                    conversations.push(conversationId);
            }
        }
    }
    return conversations;
}

async function expireReviews(home: Home, context: Context): Promise<void> {
    const jobs = (await home.harness.snapshot(ReviewsDoc, context))?.jobs ?? [];
    const retired = new Set<string>();
    for (const job of jobs) {
        const status = await (
            await home.harness.submission(job.input, context)
        )?.status(context);
        const conversations = await taskConversations(home, job.task, context);
        const active =
            status?.status === "placed" ||
            status?.status === "queued" ||
            conversations.length > 0;
        if (!job.wait) {
            if (job.done && !active) retired.add(job.task);
            continue;
        }
        const open = (await home.harness.snapshot(OversightDoc, context))
            ?.waits[job.wait.id];
        if (!open) {
            const decided = await settledReview(home, job.wait, context);
            if (!active) retired.add(job.task);
            if (decided.how !== "go" && decided.how !== "withdraw") continue;
            const key = agentKey(job.agent);
            await home.harness.commit(async (tx) => {
                const told = (
                    await tx.doc(InstructionsDoc, key, key)
                ).given.find((i) => i.task === job.task);
                if (decided.how === "go" && told) told.hearing.cleared = true;
                if (decided.how === "withdraw" && active) {
                    (
                        await tx.doc(InstructionDoc, job.task, job.task)
                    ).withdrawn =
                        `The chief withdrew the instruction: ${decided.text}`;
                    await stopTaskWaits(home, tx, new Set(conversations));
                }
                await settleOthers(home, tx, job.task, decided);
            }, context);
            if (decided.how === "withdraw" && active)
                await abortWithdrawn(home, job.conversation, job.task, context);
            continue;
        }
        if (active) continue;
        await home.harness.commit(async (tx) => {
            const wait = (await tx.doc(OversightDoc)).waits[job.wait?.id ?? ""];
            if (!wait || wait.answers.length === 1) return;
            wait.answers = ["dismiss"];
            wait.calls = [];
            wait.about += "\n(The work ended before you answered.)";
        }, context);
    }
    if (retired.size)
        await home.harness.commit(async (tx) => {
            const reviews = await tx.doc(ReviewsDoc);
            reviews.jobs = reviews.jobs.filter((job) => !retired.has(job.task));
        }, context);
}

async function settleOthers(
    home: Home,
    tx: Tx,
    task: string,
    ruled: Ruling,
): Promise<void> {
    if (ruled.how !== "go" && ruled.how !== "withdraw") return;
    for (const id of (await tx.doc(InstructionDoc, task, task)).waits) {
        await settle(
            tx,
            id,
            { how: ruled.how, text: ruled.text, chief: home.chief },
            home.now(),
        );
    }
}

function waitName(name: string, hamlet: string): string {
    return name === "mayor" && hamlet !== "home" ? `mayor@${hamlet}` : name;
}
