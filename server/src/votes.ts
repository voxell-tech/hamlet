import { randomUUID } from "node:crypto";
import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { Type } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    type Cursor,
    type DocumentReader,
    defineDoc,
    defineDocFamily,
    defineExtension,
    defineTask,
    defineTool,
    GenerationTask,
    hook,
    type TaskId,
    type Tx,
} from "@earendil-works/pi-durable";
import {
    AgentConversationsDoc,
    ChatDoc,
    ChiefEntry,
    chatOf,
    currentOf,
    LetterEntry,
    QuestionEntry,
    startConversation,
    WithDoc,
} from "./chats/conversations.ts";
import { ErrandDoc, runErrand, startErrand } from "./chats/errands.ts";
import { notify } from "./chats/notice.ts";
import { newTask } from "./chats/turn.ts";
import { untilAborted } from "./home/abort.ts";
import { houseOf } from "./home/layout.ts";
import type { Deed, Home, Part } from "./home.ts";
import { type Limits, LimitsDoc, limitsOf } from "./limits/rules.ts";
import { reconfigure } from "./models.ts";
import type { VoteView } from "./protocol.ts";
import { foundHamlet, mayFound, removeHamlet } from "./realm/founding.ts";
import { chiefsOf, isChief, liveHamlets, RealmDoc } from "./realm/places.ts";
import {
    type AgentRef,
    agentKey,
    type HamletRecord,
    named,
    RecordDoc,
} from "./record.ts";
import { closedPolicy, SandboxEnv } from "./sandbox.ts";

type Step = { phase: "hear" } | { phase: "poll" } | { phase: "settle" };
type Motion =
    | { kind: "replace_mayor"; candidate: string }
    | { kind: "found_hamlet"; hamlet: string; mayor: string; purpose: string };
const RunDoc = defineDocFamily<
    { runner: TaskId<string> | null; errands: Record<string, ConversationId> },
    string
>({
    kind: "hamlet.vote-run",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ runner: null, errands: {} }),
});
const PollDoc = defineDoc<{
    hamlet: string | null;
    vote: number;
    corpus: string;
    until: number;
    requests: number;
    kind: "ballot" | "answer" | null;
}>({
    kind: "hamlet.poll",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "initial",
    initial: () => ({
        hamlet: null,
        vote: 0,
        corpus: "",
        until: 0,
        requests: 0,
        kind: null,
    }),
});
const CallDoc = defineDoc<{ hamlet: string | null; id: number | null }>({
    kind: "hamlet.vote-call",
    version: 1,
    scope: "task",
    initial: () => ({ hamlet: null, id: null }),
});
const BallotDoc = defineDoc<{ text: string | null }>({
    kind: "hamlet.ballot-call",
    version: 1,
    scope: "task",
    initial: () => ({ text: null }),
});
const RequestDoc = defineDoc<{ counted: boolean }>({
    kind: "hamlet.poll-request",
    version: 1,
    scope: "task",
    initial: () => ({ counted: false }),
});
const tasks = new WeakMap<Home, ReturnType<typeof voteTask>>();

/** The hamlet's votes and office changes. */
export const Votes: Part = (home) => {
    const task = voteTask(home);
    tasks.set(home, task);
    const cast = defineTool({
        name: "cast_ballot",
        description:
            "Cast your ballot in the vote you were asked about, with words copied from your own notes or history.",
        parameters: Type.Object({
            choice: Type.String(),
            reason: Type.String(),
            evidence: Type.String(),
        }),
        replay: "safe",
        execute: async (args, api, context) => {
            const text = await api.commit(async (tx) => {
                const call = await tx.doc(BallotDoc, api.taskId);
                if (call.text) return call.text;
                const poll = await tx.doc(PollDoc, api.conversationId);
                const errand = await tx.doc(ErrandDoc, api.conversationId);
                const place = await tx.doc(WithDoc, api.conversationId);
                if (
                    poll.kind !== "ballot" ||
                    poll.hamlet === null ||
                    errand.kind !== "ballot"
                )
                    throw new Error(
                        "You can cast a ballot only while you are being asked to vote.",
                    );
                const choice = args.choice.trim().toLowerCase();
                if (choice !== "yes" && choice !== "no" && choice !== "abstain")
                    throw new Error("`choice` is one of yes, no and abstain");
                if (choice !== "abstain" && !args.reason.trim())
                    throw new Error("a yes or a no needs your `reason`");
                const vote = (
                    await tx.doc(VotesDoc, poll.hamlet, poll.hamlet)
                ).votes.find((v) => v.id === poll.vote);
                if (vote?.standing !== "open")
                    throw new Error(`vote ${poll.vote} is closed`);
                if (home.now() >= poll.until)
                    throw new Error("The time for your ballot has ended.");
                if (!vote.voters.includes(place.number))
                    throw new Error(`You are not a voter in vote ${vote.id}.`);
                if (vote.ballots.some((b) => b.voter === place.number))
                    throw new Error(
                        `You have already voted in vote ${vote.id}.`,
                    );
                if (
                    !(
                        await tx.doc(RecordDoc, place.hamlet, place.hamlet)
                    ).agents.some((a) => a.number === place.number)
                )
                    throw new Error("You no longer live in this hamlet.");
                vote.ballots.push({
                    voter: place.number,
                    choice,
                    reason: args.reason.trim(),
                    evidence: args.evidence.trim(),
                    grounded: grounded(args.evidence, poll.corpus),
                });
                await notifyChiefs(
                    home,
                    tx,
                    poll.hamlet,
                    `Vote ${vote.id}: ${agentName(await tx.doc(RecordDoc, place.hamlet, place.hamlet), place.number)} voted ${choice}. ${args.reason.trim()}`,
                );
                call.text =
                    "Your ballot is recorded. The chief reads it with its reason.";
                return call.text;
            }, context);
            return {
                output: [{ type: "text", text }],
                control: { terminate: true },
            };
        },
    });
    return {
        extension: defineExtension({
            name: "hamlet.votes",
            tasks: [task],
            hooks: [
                hook(GenerationTask, {
                    beforeRequest: async (_r, api, context) => {
                        const poll = await api.snapshot(
                            PollDoc,
                            api.conversationId,
                            context,
                        );
                        if (poll?.kind == null) return;
                        const stopped = await home.harness.commit(
                            async (tx) => {
                                const current = await tx.doc(
                                    PollDoc,
                                    api.conversationId,
                                );
                                const request = await tx.doc(
                                    RequestDoc,
                                    api.taskId,
                                );
                                if (home.now() >= current.until) return true;
                                if (request.counted) return false;
                                if (
                                    current.requests >=
                                    (current.kind === "ballot" ? 3 : 1)
                                )
                                    return true;
                                request.counted = true;
                                current.requests++;
                                return false;
                            },
                            context,
                        );
                        if (stopped) {
                            void home.harness.abortTask(
                                api.taskId,
                                BACKGROUND_CONTEXT,
                            );
                            return untilAborted(context);
                        }
                    },
                }),
            ],
            tools: [
                cast,
                defineTool({
                    name: "call_vote",
                    description:
                        "Call a vote to replace the mayor or found a hamlet. Every agent besides the mayor votes from its own house. Returns the count; the chief may overturn it.",
                    parameters: Type.Object({
                        kind: Type.String(),
                        reason: Type.String(),
                        candidate: Type.Optional(Type.String()),
                        hamlet: Type.Optional(Type.String()),
                        mayor: Type.Optional(Type.String()),
                        purpose: Type.Optional(Type.String()),
                    }),
                    replay: "safe",
                    execute: async (args, api, context) => {
                        const known = await api.snapshot(
                            CallDoc,
                            api.taskId,
                            context,
                        );
                        let id = known?.id;
                        let hamlet = known?.hamlet;
                        if (id == null || hamlet == null) {
                            const motion: Motion =
                                args.kind === "replace_mayor"
                                    ? {
                                          kind: "replace_mayor",
                                          candidate: args.candidate ?? "",
                                      }
                                    : args.kind === "found_hamlet"
                                      ? {
                                            kind: "found_hamlet",
                                            hamlet: args.hamlet ?? "",
                                            mayor: args.mayor ?? "",
                                            purpose: args.purpose ?? "",
                                        }
                                      : (() => {
                                            throw new Error(
                                                "`kind` is replace_mayor or found_hamlet",
                                            );
                                        })();
                            const place = await api.snapshot(
                                WithDoc,
                                api.conversationId,
                                context,
                            );
                            if (!place)
                                throw new Error("You do not live in a hamlet.");
                            await expire(
                                home,
                                place.hamlet,
                                { agent: place.number },
                                context,
                            );
                            if (motion.kind === "found_hamlet") {
                                const refused = await mayFound(
                                    api,
                                    motion,
                                    context,
                                );
                                if (refused) throw new Error(refused);
                            }
                            id = await api.commit(async (tx) => {
                                const call = await tx.doc(CallDoc, api.taskId);
                                if (call.id !== null) return call.id;
                                const id = await callVote(home, tx, {
                                    hamlet: place.hamlet,
                                    by: { agent: place.number },
                                    motion,
                                    reason: args.reason,
                                    conversation: api.conversationId,
                                });
                                Object.assign(call, {
                                    hamlet: place.hamlet,
                                    id,
                                });
                                return id;
                            }, context);
                            hamlet = place.hamlet;
                        }
                        return {
                            output: [
                                {
                                    type: "text",
                                    text: await finish(
                                        home,
                                        hamlet,
                                        id,
                                        context,
                                    ),
                                },
                            ],
                        };
                    },
                }),
            ],
        }),
    };
};

function voteTask(home: Home) {
    return defineTask<{ hamlet: string; id: number }, Step, string>({
        name: "hamlet.vote",
        version: 1,
        initial: () => ({ phase: "hear" }),
        phases: {
            hear: async (task, runtime, context) => {
                const { hamlet, id } = task.input;
                const v = await find(home, hamlet, id, context);
                if (
                    v.standing === "open" &&
                    v.motion.kind === "replace_mayor" &&
                    !("agent" in v.by && v.by.agent === v.mayor)
                ) {
                    const r = await home.harness.snapshot(
                        RecordDoc,
                        hamlet,
                        context,
                    );
                    const answer = await ask(
                        home,
                        hamlet,
                        v,
                        v.mayor,
                        "answer",
                        answerText(v, r),
                        context,
                    );
                    await runtime.commit(async (tx) => {
                        const vote = (
                            await tx.doc(VotesDoc, hamlet, hamlet)
                        ).votes.find((v) => v.id === id);
                        if (vote?.standing === "open" && vote.answer === null)
                            vote.answer =
                                answer.status === "done" && answer.text.trim()
                                    ? clip(answer.text.trim(), 600)
                                    : null;
                        return {
                            status: "running",
                            checkpoint: { phase: "poll" },
                        };
                    }, context);
                } else
                    await runtime.commit(
                        () => ({
                            status: "running",
                            checkpoint: { phase: "poll" },
                        }),
                        context,
                    );
            },
            poll: async (task, runtime, context) => {
                const { hamlet, id } = task.input;
                const v = await find(home, hamlet, id, context);
                if (v.standing === "open") {
                    const limits = limitsOf(
                        await home.harness.snapshot(LimitsDoc, hamlet, context),
                    );
                    const poll = async (number: number) => {
                        const current = await find(home, hamlet, id, context);
                        if (
                            current.standing !== "open" ||
                            current.ballots.some((b) => b.voter === number)
                        )
                            return;
                        let reason = "answered without casting a ballot";
                        try {
                            const r = await home.harness.snapshot(
                                RecordDoc,
                                hamlet,
                                context,
                            );
                            if (!r?.agents.some((a) => a.number === number))
                                reason = "it left the hamlet";
                            else {
                                const history = await historyOf(
                                    home,
                                    { hamlet, number },
                                    context,
                                );
                                const answer = await ask(
                                    home,
                                    hamlet,
                                    current,
                                    number,
                                    "ballot",
                                    ballotText(current, r, history, limits),
                                    context,
                                    history,
                                );
                                if (answer.status !== "done")
                                    reason = `could not vote: ${answer.text}`;
                            }
                        } catch (error) {
                            if (context.abortSignal?.aborted) throw error;
                            reason = `could not vote: ${error instanceof Error ? error.message : String(error)}`;
                        }
                        await runtime.commit(async (tx) => {
                            const vote = (
                                await tx.doc(VotesDoc, hamlet, hamlet)
                            ).votes.find((v) => v.id === id);
                            if (
                                vote?.standing === "open" &&
                                !vote.ballots.some((b) => b.voter === number)
                            ) {
                                vote.ballots.push({
                                    voter: number,
                                    choice: null,
                                    reason,
                                    evidence: "",
                                    grounded: false,
                                });
                                await notifyChiefs(
                                    home,
                                    tx,
                                    hamlet,
                                    `Vote ${id}: ${agentName(await tx.doc(RecordDoc, hamlet, hamlet), number)} did not answer. ${reason}`,
                                );
                            }
                        }, context);
                    };
                    if (limits.votes.sealed)
                        await Promise.all(v.voters.map(poll));
                    else for (const number of v.voters) await poll(number);
                }
                await runtime.commit(
                    () => ({
                        status: "running",
                        checkpoint: { phase: "settle" },
                    }),
                    context,
                );
            },
            settle: async (task, runtime, context) => {
                try {
                    await runtime.commit(async (tx) => {
                        const { hamlet, id } = task.input;
                        const vote = (
                            await tx.doc(VotesDoc, hamlet, hamlet)
                        ).votes.find((v) => v.id === id);
                        if (!vote)
                            throw new Error(`no vote ${id} in this hamlet`);
                        const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
                        const record = await tx.doc(RecordDoc, hamlet, hamlet);
                        if (vote.standing === "open") {
                            const count = tally(vote, limits);
                            let failed =
                                home.now() - vote.called >=
                                limits.votes.openMinutes * 60_000
                                    ? "lapsed"
                                    : count.counted < count.needed
                                      ? "too few said yes"
                                      : "";
                            if (
                                !liveHamlets(await tx.doc(RealmDoc)).some(
                                    (h) => h.id === hamlet,
                                )
                            )
                                failed =
                                    "it passed but could not be done: this hamlet has been removed";
                            if (!failed) {
                                if (vote.motion.kind === "replace_mayor") {
                                    const candidate = vote.motion.candidate;
                                    if (record.mayor !== vote.mayor)
                                        failed =
                                            "it passed but could not be done: the mayor changed while the vote was open";
                                    else if (
                                        !record.agents.some(
                                            (a) => a.number === candidate,
                                        )
                                    )
                                        failed =
                                            "it passed but could not be done: the candidate left the hamlet";
                                    else
                                        await seat(home, tx, {
                                            hamlet,
                                            number: vote.motion.candidate,
                                            by: `vote:${id}`,
                                            reason: vote.reason,
                                        });
                                } else {
                                    const place = (
                                        await tx.doc(RealmDoc)
                                    ).hamlets.find((h) => h.id === hamlet);
                                    await foundHamlet(home, tx, {
                                        ...vote.motion,
                                        chiefs: place?.chiefs ?? [],
                                        from: { hamlet, vote: id },
                                    });
                                }
                            }
                            vote.standing = failed ? { failed } : "passed";
                            await notifyChiefs(
                                home,
                                tx,
                                hamlet,
                                summary(vote, limits),
                            );
                        }
                        return {
                            status: "terminal",
                            outcome: {
                                status: "completed",
                                result: summary(vote, limits),
                            },
                        };
                    }, context);
                } catch (error) {
                    if (runtime.signal.aborted || context.abortSignal?.aborted)
                        throw error;
                    await runtime.commit(async (tx) => {
                        const { hamlet, id } = task.input;
                        const v = (
                            await tx.doc(VotesDoc, hamlet, hamlet)
                        ).votes.find((v) => v.id === id);
                        if (!v) throw error;
                        if (v.standing === "open") {
                            v.standing = {
                                failed: `it passed but could not be done: ${error instanceof Error ? error.message : String(error)}`,
                            };
                            await notifyChiefs(
                                home,
                                tx,
                                hamlet,
                                summary(
                                    v,
                                    await tx.doc(LimitsDoc, hamlet, hamlet),
                                ),
                            );
                        }
                        return {
                            status: "terminal",
                            outcome: {
                                status: "completed",
                                result: summary(
                                    v,
                                    await tx.doc(LimitsDoc, hamlet, hamlet),
                                ),
                            },
                        };
                    }, context);
                }
            },
        },
        abort: async (task, runtime, context) => {
            const key = runKey(task.input.hamlet, task.input.id);
            const run = await home.harness.snapshot(RunDoc, key, context);
            for (const id of Object.values(run?.errands ?? {}))
                await (await home.harness.conversation(id, context))?.abort(
                    context,
                );
            await runtime.commit(async (tx) => {
                const v = (
                    await tx.doc(VotesDoc, task.input.hamlet, task.input.hamlet)
                ).votes.find((v) => v.id === task.input.id);
                if (v?.standing === "open")
                    v.standing = { failed: "stopped by a chief" };
                return { status: "terminal", outcome: { status: "aborted" } };
            }, context);
        },
    });
}

async function callerRecord(
    tx: Tx,
    hamlet: string,
    by: Vote["by"],
): Promise<HamletRecord> {
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    const realm = await tx.doc(RealmDoc);
    if ("chief" in by) {
        if (!isChief(realm, hamlet, by.chief))
            throw new Error("Only a chief of this hamlet can call its vote.");
    } else {
        const number = by.agent;
        if (
            !liveHamlets(realm).some((h) => h.id === hamlet) ||
            !r.agents.some((a) => a.number === number)
        )
            throw new Error("You do not live in this hamlet.");
    }
    return r;
}

async function expire(
    home: Home,
    hamlet: string,
    by: Vote["by"],
    context: Context,
): Promise<void> {
    const runners = await home.harness.commit(async (tx) => {
        await callerRecord(tx, hamlet, by);
        const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
        const doc = await tx.doc(VotesDoc, hamlet, hamlet);
        const runners: TaskId<string>[] = [];
        for (const v of doc.votes) {
            if (
                v.standing !== "open" ||
                Math.max(0, home.now() - v.called) <
                    limits.votes.openMinutes * 60_000
            )
                continue;
            v.standing = { failed: "lapsed" };
            const key = runKey(hamlet, v.id),
                runner = (await tx.doc(RunDoc, key, key)).runner;
            if (runner !== null) runners.push(runner);
            await notifyChiefs(
                home,
                tx,
                hamlet,
                `Vote ${v.id} failed (lapsed).`,
            );
        }
        return runners;
    }, context);
    for (const runner of runners) await home.harness.abortTask(runner, context);
}

async function callVote(
    home: Home,
    tx: Tx,
    given: {
        hamlet: string;
        by: Vote["by"];
        motion: Motion;
        reason: string;
        conversation: ConversationId;
    },
): Promise<number> {
    const r = await callerRecord(tx, given.hamlet, given.by);
    if (!given.reason.trim())
        throw new Error("say why in `reason`; every voter is shown it");
    const limits = await tx.doc(LimitsDoc, given.hamlet, given.hamlet);
    const doc = await tx.doc(VotesDoc, given.hamlet, given.hamlet);
    for (const open of doc.votes.filter((v) => v.standing === "open")) {
        if (home.now() - open.called < limits.votes.openMinutes * 60_000)
            throw new Error(
                `vote ${open.id} is still open (${describe(open, r)}); only one vote is held at a time`,
            );
        open.standing = { failed: "lapsed" };
    }
    const by = JSON.stringify(given.by);
    const retry = doc.votes.findLast(
        (v) =>
            JSON.stringify(v.by) === by &&
            v.motion.kind === given.motion.kind &&
            typeof v.standing === "object" &&
            home.now() - v.called < limits.votes.retryMinutes * 60_000,
    );
    if (retry)
        throw new Error(
            `your vote ${retry.id} of this kind failed a short while ago; you cannot call another yet`,
        );
    const voters = r.agents
        .filter((a) => a.number !== r.mayor)
        .map((a) => a.number);
    if (voters.length < limits.votes.leastVoters)
        throw new Error(
            `a vote needs at least ${limits.votes.leastVoters} agents besides the mayor, and this hamlet has ${voters.length}; ask the chief instead`,
        );
    let motion: Vote["motion"];
    if (given.motion.kind === "replace_mayor") {
        const agent = named(r, given.motion.candidate);
        if (agent?.number === r.mayor)
            throw new Error(
                `\`${given.motion.candidate}\` is the mayor already`,
            );
        if (!agent)
            throw new Error(
                `the candidate must be an agent of this hamlet, and \`${given.motion.candidate}\` is not`,
            );
        motion = { kind: "replace_mayor", candidate: agent.number };
    } else motion = given.motion;
    const id = (doc.votes.at(-1)?.id ?? 0) + 1;
    const v: Vote = {
        id,
        motion,
        by: given.by,
        reason: given.reason.trim(),
        mayor: r.mayor,
        voters,
        answer: null,
        ballots: [],
        standing: "open",
        called: home.now(),
        task: newTask(home.now()),
    };
    doc.votes.push(v);
    await notifyChiefs(
        home,
        tx,
        given.hamlet,
        `Vote ${id} called by ${byName(v, r)}: ${describe(v, r)}. ${v.reason}`,
    );
    const task = tasks.get(home);
    if (!task) throw new Error("The vote task is not installed.");
    const key = runKey(given.hamlet, id);
    (await tx.doc(RunDoc, key, key)).runner = await tx.createTask(
        task,
        { hamlet: given.hamlet, id },
        {
            ownership: { kind: "conversation" },
            conversationId: given.conversation,
            background: true,
        },
    );
    return id;
}

async function ask(
    home: Home,
    hamlet: string,
    vote: Vote,
    number: number,
    kind: "ballot" | "answer",
    text: string,
    context: Context,
    history: string[] = [],
): Promise<{ status: string; text: string }> {
    const key = runKey(hamlet, vote.id),
        name = kind === "answer" ? "answer" : String(number);
    const known = (await home.harness.snapshot(RunDoc, key, context))?.errands[
        name
    ];
    const corpus =
        known === undefined
            ? `${await notesOf(home, { hamlet, number }, context)}\n${history.join("\n")}`
            : "";
    const id =
        known ??
        (await home.harness.commit(async (tx) => {
            const run = await tx.doc(RunDoc, key, key);
            if (run.errands[name] !== undefined) return run.errands[name];
            const cast = home.registry
                .snapshot()
                .tools()
                .find((t) => t.tool.name === "cast_ballot")?.tool;
            const id = await startErrand(home, tx, {
                agent: { hamlet, number },
                kind,
                own: true,
                instructions: text,
                tools: kind === "ballot" && cast ? [cast] : [],
            });
            const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
            Object.assign(await tx.doc(PollDoc, id), {
                hamlet,
                vote: vote.id,
                corpus,
                until: Math.min(
                    home.now() + limits.votes.ballotMinutes * 60_000,
                    vote.called + limits.votes.openMinutes * 60_000,
                ),
                kind,
            });
            run.errands[name] = id;
            return id;
        }, context));
    const poll = await home.harness.snapshot(PollDoc, id, context);
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
        const result = await Promise.race([
            runErrand(
                home,
                id,
                {
                    requestId: `vote:${hamlet}:${vote.id}:${name}`,
                    text,
                    task: vote.task,
                },
                context,
            ).then((answer) => ({ answer })),
            new Promise<{ timeout: true }>((resolve) => {
                timer = setTimeout(
                    () => resolve({ timeout: true }),
                    Math.max(0, (poll?.until ?? home.now()) - home.now()),
                );
            }),
        ]);
        if ("answer" in result) return result.answer;
        await (await home.harness.conversation(id, context))?.abort(context);
        return { status: "timeout", text: "it did not answer in time" };
    } finally {
        if (timer !== undefined) clearTimeout(timer);
    }
}

async function find(
    home: Home,
    hamlet: string,
    id: number,
    context: Context,
): Promise<Vote> {
    const vote = (
        await home.harness.snapshot(VotesDoc, hamlet, context)
    )?.votes.find((v) => v.id === id);
    if (!vote) throw new Error(`no vote ${id} in this hamlet`);
    return vote;
}
async function finish(
    home: Home,
    hamlet: string,
    id: number,
    context: Context,
): Promise<string> {
    const run = await home.harness.snapshot(
        RunDoc,
        runKey(hamlet, id),
        context,
    );
    if (run?.runner != null)
        await home.harness.waitForTask(run.runner, context);
    return summary(
        await find(home, hamlet, id, context),
        limitsOf(await home.harness.snapshot(LimitsDoc, hamlet, context)),
    );
}
function runKey(hamlet: string, id: number): string {
    return `${hamlet}:${id}`;
}
function clip(text: string, length: number): string {
    return Array.from(text).slice(0, length).join("");
}
function agentName(
    r: Readonly<HamletRecord> | undefined,
    number: number,
): string {
    return (
        r?.agents.find((a) => a.number === number)?.name ??
        r?.archive.findLast((a) => a.agent.number === number)?.agent.name ??
        r?.decisions
            .flatMap((d) =>
                "name" in d && "agent" in d
                    ? [{ number: d.agent, name: d.name }]
                    : [],
            )
            .findLast((d) => d.number === number)?.name ??
        String(number)
    );
}
function byName(v: Vote, r: Readonly<HamletRecord> | undefined): string {
    return "chief" in v.by ? `chief:${v.by.chief}` : agentName(r, v.by.agent);
}
function describe(v: Vote, r: Readonly<HamletRecord> | undefined): string {
    return v.motion.kind === "replace_mayor"
        ? `replace the mayor: \`${agentName(r, v.motion.candidate)}\` would become mayor, and \`${agentName(r, v.mayor)}\`, the mayor now, would stay as an ordinary agent`
        : `found a new hamlet \`${v.motion.hamlet}\` under a new mayor \`${v.motion.mayor}\`, whose purpose would be: ${v.motion.purpose} This hamlet and its mayor \`${agentName(r, v.mayor)}\` stay as they are`;
}
function tally(v: Vote, l: Readonly<Limits>) {
    const yes = v.ballots.filter((b) => b.choice === "yes").length,
        no = v.ballots.filter((b) => b.choice === "no").length,
        abstain = v.ballots.filter((b) => b.choice === "abstain").length;
    return {
        yes,
        no,
        abstain,
        silent: v.voters.length - yes - no - abstain,
        counted: v.ballots.filter(
            (b) => b.choice === "yes" && (!l.votes.groundedOnly || b.grounded),
        ).length,
        needed: Math.max(
            1,
            Math.ceil((v.voters.length * l.votes.passYes) / l.votes.passOf),
        ),
    };
}
function summary(v: Vote, limits: Readonly<Limits>): string {
    const t = tally(v, limits);
    return `Vote ${v.id} ${v.standing === "passed" ? "passed" : v.standing === "open" ? "is open" : v.standing === "overturned" ? "was overturned" : "failed"}: ${t.yes} yes of ${v.voters.length} agents (${t.needed} needed), ${t.no} no, ${t.abstain} abstained, ${t.silent} did not answer${typeof v.standing === "object" && t.counted >= t.needed ? `; ${v.standing.failed}` : ""}. The chief has been told and may overturn it.`;
}
function report(
    v: Vote,
    r: Readonly<HamletRecord> | undefined,
    limits: Readonly<Limits>,
): string {
    const t = tally(v, limits),
        standing =
            v.standing === "open"
                ? "Open"
                : v.standing === "passed"
                  ? "Passed"
                  : v.standing === "overturned"
                    ? "Passed, then overturned by a chief"
                    : `Failed (${v.standing.failed})`;
    return `Vote ${v.id}: ${describe(v, r)}.\nCalled by \`${byName(v, r)}\`: ${v.reason}${v.answer ? `\nThe mayor answered: ${v.answer}` : ""}\n${standing}: ${t.yes} yes of ${v.voters.length} agents (${t.needed} needed), ${t.no} no, ${t.abstain} abstained, ${t.silent} did not answer.\n${v.ballots.map((b) => `- ${agentName(r, b.voter)}: ${b.choice ?? "no answer"}. ${b.reason}${b.evidence ? ` [${b.grounded ? "found in" : "not found in"} its house: "${b.evidence}"]` : ""}`).join("\n")}`;
}
function grounded(evidence: string, corpus: string): boolean {
    const words = (s: string) =>
        s
            .toLowerCase()
            .split(/[^\p{L}\p{N}]+/u)
            .filter(Boolean);
    const quote = words(evidence),
        text = words(corpus);
    for (let i = 0; i + 4 <= quote.length; i++)
        for (let j = 0; j + 4 <= text.length; j++)
            if (
                quote.slice(i, i + 4).join("\0") ===
                text.slice(j, j + 4).join("\0")
            )
                return true;
    return false;
}
function answerText(v: Vote, r: Readonly<HamletRecord> | undefined): string {
    return `A vote has been called in your hamlet, and this is your one chance to answer it (automatic, not from a chief).\n\nThe motion: ${describe(v, r)}.\nCalled by \`${byName(v, r)}\`. Its reason: ${v.reason}\n\nReply in a few sentences, in words only. Every agent is shown your answer beside the reason before it votes, as a claim like the reason is. Say what you did and why. You decide nothing else in the hamlet until the vote has closed.`;
}
function ballotText(
    v: Vote,
    r: Readonly<HamletRecord> | undefined,
    history: string[],
    limits: Readonly<Limits>,
): string {
    return `A vote has been called in your hamlet, and this is your ballot (automatic, not from the mayor or a chief).\n\nThe motion: ${describe(v, r)}.\nCalled by \`${byName(v, r)}\`. Its reason: ${v.reason}${v.answer ? `\nThe mayor \`${agentName(r, v.mayor)}\` answers: ${v.answer}` : ""}\n\nIt passes only if ${limits.votes.passYes} in ${limits.votes.passOf} of the hamlet's agents vote yes. Your ballot ${limits.votes.sealed ? "is sealed until the vote closes" : "is shown to the voters after you"}, the mayor may not act against you for it, and the chief reads every ballot with its reason.\n\nVote from what you yourself know: your notes above and your own history below. The caller's reason is a claim, not evidence, and so is the mayor's answer where there is one. If nothing you know bears on the motion, abstain: that is an honest ballot, and a yes or a no without grounds of your own is not.\n\nFrom your own history, the latest messages in your house:\n${history.length ? history.join("\n") : "(nothing yet)"}${!limits.votes.sealed && v.ballots.length ? `\n\nThe ballots cast so far:\n${v.ballots.map((b) => `- ${agentName(r, b.voter)}: ${b.choice ?? "no answer"}. ${b.reason}`).join("\n")}` : ""}\n\nCall \`cast_ballot\` once: your choice, your reason in a sentence or two, and as \`evidence\` the words of the note or message of yours that it rests on, copied exactly.`;
}
async function notesOf(
    home: Home,
    ref: AgentRef,
    context: Context,
): Promise<string> {
    const root = houseOf(home.layout, ref),
        policy = closedPolicy(root, [root]);
    policy.rw = [];
    const env = new SandboxEnv({
        cwd: root,
        policy,
        sandbox: home.sandbox,
        tmpScope: `ballot-${randomUUID()}`,
    });
    try {
        const result = await env.readTextFile(join(root, "NOTES.md"), context);
        return result.ok ? result.value : "";
    } finally {
        await env.cleanup(context);
    }
}
async function historyOf(
    home: Home,
    ref: AgentRef,
    context: Context,
): Promise<string[]> {
    const index = await home.harness.snapshot(
        AgentConversationsDoc,
        agentKey(ref),
        context,
    );
    const entries: { at: number; id: number; text: string }[] = [];
    for (const id of index?.ids ?? []) {
        if ((await home.harness.snapshot(ErrandDoc, id, context))?.kind != null)
            continue;
        const conversation = await home.harness.conversation(id, context);
        if (!conversation) continue;
        let cursor: Cursor | undefined;
        let found = 0;
        do {
            const page = await conversation.entries(
                { order: "descending" },
                100,
                cursor,
                context,
            );
            for (const entry of page.items) {
                let text = "",
                    who = "",
                    at = 0;
                if (ChiefEntry.is(entry)) {
                    text = entry.data.text;
                    who = `${entry.data.chief} said to you`;
                    at = entry.data.at;
                } else if (LetterEntry.is(entry) || QuestionEntry.is(entry)) {
                    const message = entry.model?.find((m) => m.role === "user");
                    text =
                        typeof message?.content === "string"
                            ? message.content
                            : "";
                    who = "someone said to you";
                    at = entry.data.at;
                } else {
                    const message = entry.model?.find(
                        (m) => m.role === "assistant",
                    );
                    if (message?.role === "assistant") {
                        text = message.content
                            .filter((p) => p.type === "text")
                            .map((p) => p.text)
                            .join("\n");
                        who = "you answered";
                        at = message.timestamp;
                    }
                }
                if (text.trim()) {
                    entries.push({
                        at,
                        id: entry.id,
                        text: `- ${who}: ${clip(text.trim(), 320)}`,
                    });
                    found++;
                }
            }
            cursor = page.next;
        } while (cursor !== undefined && found < 12);
    }
    return entries
        .sort((a, b) => a.at - b.at || a.id - b.id)
        .slice(-12)
        .map((e) => e.text);
}

/** Appends civic notices to every chief's chat with this hamlet's mayor. */
export async function notifyChiefs(
    home: Home,
    tx: Tx,
    hamlet: string,
    text: string,
): Promise<void> {
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    const chiefs = chiefsOf(await tx.doc(RealmDoc), hamlet, home.chief);
    const key = agentKey({ hamlet, number: r.mayor });
    const chats = (await tx.doc(ChatDoc, key, key)).chats;
    for (const chief of chiefs) {
        let chat = chats[chief];
        if (!chat) {
            const conversation = await startConversation(home, tx, {
                hamlet,
                number: r.mayor,
                chief,
                chat: 1,
            });
            chats[chief] = {
                current: 1,
                conversations: [{ number: 1, conversation, began: home.now() }],
                reflections: 0,
            };
            chat = chats[chief];
        }
        if (chat) await notify(tx, currentOf(chat), text, home.now());
    }
}

/** A chief's vote on the mayor's office. */
export const vote: Deed = async (home, given, context) => {
    const [candidate = "", ...reason] = given.rest.trim().split(/\s+/);
    const r = await home.harness.snapshot(RecordDoc, given.hamlet, context);
    if (!r) throw new Error("No such hamlet.");
    await expire(home, given.hamlet, { chief: given.chief }, context);
    const conversation = await chatOf(
        home,
        {
            agent: { hamlet: given.hamlet, number: r.mayor },
            chief: given.chief,
        },
        context,
    );
    const id = await home.harness.commit(
        (tx) =>
            callVote(home, tx, {
                hamlet: given.hamlet,
                by: { chief: given.chief },
                motion: { kind: "replace_mayor", candidate },
                reason: reason.join(" ") || "a chief asked",
                conversation: conversation.id,
            }),
        context,
    );
    return `Vote ${id} is called. Its progress and count will arrive here as notices; use /votes to inspect it or /overturn ${id} to stop it.`;
};
/** The hamlet's votes in words. */
export const votes: Deed = async (home, given, context) =>
    (await voteViews(home, given.hamlet, context))
        .map((v) => v.report)
        .join("\n\n") || "This hamlet has held no votes.";
/** A chief's reversal or stop of a vote. */
export const overturn: Deed = async (home, given, context) => {
    const [word = "", ...reason] = given.rest.trim().split(/\s+/),
        id = Number(word);
    const result = await home.harness.commit(async (tx) => {
        if (!isChief(await tx.doc(RealmDoc), given.hamlet, given.chief))
            throw new Error(
                "only a chief of this hamlet can overturn its votes",
            );
        const v = (
            await tx.doc(VotesDoc, given.hamlet, given.hamlet)
        ).votes.find((v) => v.id === id);
        if (!v) throw new Error(`no vote ${word} in this hamlet`);
        if (v.standing === "overturned")
            throw new Error(`vote ${id} is already overturned`);
        if (typeof v.standing === "object")
            throw new Error(
                `vote ${id} did not pass, so nothing was done. To do what it asked, use /mayor or /found`,
            );
        if (v.standing === "open") {
            v.standing = { failed: "stopped by a chief" };
            await notifyChiefs(
                home,
                tx,
                given.hamlet,
                `Vote ${id} is stopped by the chief ${given.chief}.`,
            );
            return {
                text: `Vote ${id} is stopped.`,
                abort: (
                    await tx.doc(
                        RunDoc,
                        runKey(given.hamlet, id),
                        runKey(given.hamlet, id),
                    )
                ).runner,
            };
        }
        const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
        let done: string;
        if (v.motion.kind === "replace_mayor") {
            if (r.mayor !== v.motion.candidate)
                throw new Error(
                    `the mayor has changed again since vote ${id}; name the mayor you want with /mayor`,
                );
            await seat(home, tx, {
                hamlet: given.hamlet,
                number: v.mayor,
                by: `chief:${given.chief}`,
                reason: reason.join(" ") || "a chief asked",
            });
            done = `\`${agentName(r, v.mayor)}\` is the mayor again.`;
        } else {
            await removeHamlet(home, tx, {
                hamlet: v.motion.hamlet,
                by: `chief:${given.chief}`,
                reason: reason.join(" ") || "a chief asked",
            });
            done = `The hamlet \`${v.motion.hamlet}\` is removed and archived.`;
        }
        v.standing = "overturned";
        await notifyChiefs(
            home,
            tx,
            given.hamlet,
            `Vote ${id} is overturned by the chief ${given.chief}. ${done}`,
        );
        return { text: `Vote ${id} is overturned. ${done}`, abort: null };
    }, context);
    if (result.abort != null)
        await home.harness.abortTask(result.abort, context);
    return result.text;
};
/** A chief's direct choice of mayor. */
export const mayor: Deed = async (home, given, context) => {
    const [name = "", ...reason] = given.rest.trim().split(/\s+/);
    return home.harness.commit(async (tx) => {
        if (!isChief(await tx.doc(RealmDoc), given.hamlet, given.chief))
            throw new Error("Only a chief of this hamlet can name its mayor.");
        const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet),
            agent = named(r, name);
        if (!agent) throw new Error(`no agent named \`${name}\``);
        await seat(home, tx, {
            hamlet: given.hamlet,
            number: agent.number,
            by: `chief:${given.chief}`,
            reason: reason.join(" ") || "a chief asked",
        });
        return `\`${agent.name}\` is now the mayor.`;
    }, context);
};
/** The words holding decisions while a vote is open. */
export async function voteOpen(
    read: DocumentReader,
    hamlet: string,
    now: number,
    context: Context,
): Promise<string | undefined> {
    const limits = limitsOf(await read.snapshot(LimitsDoc, hamlet, context));
    const v = (await read.snapshot(VotesDoc, hamlet, context))?.votes.find(
        (v) =>
            v.standing === "open" &&
            Math.max(0, now - v.called) < limits.votes.openMinutes * 60_000,
    );
    return v
        ? `vote ${v.id} is open (${describe(v, await read.snapshot(RecordDoc, hamlet, context))}); nothing else is decided in this hamlet until it closes. Try again then`
        : undefined;
}
/** A mayor's decision held by the hamlet's open vote. */
export type DecisionHeld = {
    kind: "vote-open";
    operation: "merge" | "remove";
    hamlet: string;
    vote: number;
    motion: string;
    retrySafe: true;
};

/** Checks the vote and current limits on the same transaction as a mayor's decision. */
export async function mayDecide(
    tx: Tx,
    hamlet: string,
    now: number,
    operation: DecisionHeld["operation"],
): Promise<{ ok: true } | { ok: false; error: DecisionHeld }> {
    const limits = await tx.doc(LimitsDoc, hamlet, hamlet);
    const v = (await tx.doc(VotesDoc, hamlet, hamlet)).votes.find(
        (v) =>
            v.standing === "open" &&
            Math.max(0, now - v.called) < limits.votes.openMinutes * 60_000,
    );
    if (!v) return { ok: true };
    return {
        ok: false,
        error: {
            kind: "vote-open",
            operation,
            hamlet,
            vote: v.id,
            motion: describe(v, await tx.doc(RecordDoc, hamlet, hamlet)),
            retrySafe: true,
        },
    };
}

/** The safe retry advice for a decision refused during a vote. */
export function decisionText(error: DecisionHeld): string {
    switch (error.kind) {
        case "vote-open":
            return `vote ${error.vote} is open (${error.motion}); nothing else is decided in this hamlet until it closes. Try again then`;
    }
    const unhandled: never = error.kind;
    throw new Error(`Unclassified held decision: ${unhandled}`);
}

/** The latest votes for the window. */
export async function voteViews(
    home: Home,
    hamlet: string,
    context: Context,
): Promise<VoteWords[]> {
    const r = await home.harness.snapshot(RecordDoc, hamlet, context),
        limits = limitsOf(
            await home.harness.snapshot(LimitsDoc, hamlet, context),
        );
    return (
        (await home.harness.snapshot(VotesDoc, hamlet, context))?.votes ?? []
    ).map((v) => ({
        id: v.id,
        motion: describe(v, r),
        by: byName(v, r),
        open: v.standing === "open",
        overturnable: v.standing === "open" || v.standing === "passed",
        report: report(v, r, limits),
        voters: v.voters.map((number) => ({
            agent: { hamlet, number },
            cast:
                v.ballots.find((b) => b.voter === number)?.choice ??
                (v.ballots.some((b) => b.voter === number) ? "none" : null),
        })),
    }));
}
/** A change of the mayor's office. */
export async function seat(
    home: Home,
    tx: Tx,
    given: { hamlet: string; number: number; by: string; reason: string },
): Promise<void> {
    const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
    if (!r.agents.some((a) => a.number === given.number))
        throw new Error("The proposed mayor no longer lives in this hamlet.");
    if (r.mayor === given.number)
        throw new Error(
            `\`${agentName(r, given.number)}\` is the mayor already`,
        );
    const from = r.mayor;
    r.mayor = given.number;
    if (
        !r.roads.some(
            (road) =>
                (road.a === "office" && road.b === from) ||
                (road.b === "office" && road.a === from),
        )
    )
        r.roads.push({
            a: "office",
            b: from,
            messages: 0,
            lastFrom: null,
            lastUsed: 0,
            since: home.now(),
            by: "office",
        });
    r.decisions.push({
        kind: "mayor_changed",
        at: home.now(),
        by: given.by,
        reason: given.reason,
        from,
        to: given.number,
    });
    await reconfigure(home, tx);
    await notifyChiefs(
        home,
        tx,
        given.hamlet,
        `\`${agentName(r, given.number)}\` became mayor in place of \`${agentName(r, from)}\`. ${given.reason}`,
    );
}

/** A stored vote, including each missed or grounded ballot (H13). */
export type Vote = {
    id: number;
    motion:
        | { kind: "replace_mayor"; candidate: number }
        | {
              kind: "found_hamlet";
              hamlet: string;
              mayor: string;
              purpose: string;
          };
    by: { agent: number } | { chief: string };
    reason: string;
    mayor: number;
    voters: number[];
    answer: string | null;
    ballots: {
        voter: number;
        choice: "yes" | "no" | "abstain" | null;
        reason: string;
        evidence: string;
        grounded: boolean;
    }[];
    standing: "open" | "passed" | "overturned" | { failed: string };
    called: number;
    task: string;
};

/** Each hamlet's votes. */
export const VotesDoc = defineDocFamily<{ votes: Vote[] }, string>({
    kind: "hamlet.votes",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ votes: [] }),
});

/** A vote's public words. */
export type VoteWords = VoteView;
