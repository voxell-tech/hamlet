import type { Context } from "@earendil-works/chord";
import {
    type ConversationId,
    type Cursor,
    defineDoc,
    defineDocFamily,
    defineTask,
    type TaskId,
    type Tx,
} from "@earendil-works/pi-durable";
import { clip } from "../change.ts";
import { reconfigure } from "../chats/configure.ts";
import {
    AgentConversationsDoc,
    ChiefEntry,
    LetterEntry,
    QuestionEntry,
    tellChiefs,
} from "../chats/conversations.ts";
import {
    ErrandDoc,
    type ErrandOutcome,
    runErrand,
    startErrand,
} from "../chats/errands.ts";
import type { Home } from "../home.ts";
import { LimitsDoc, limitsOf } from "../limits/rules.ts";
import { liveHamlets, RealmDoc } from "../realm/places.ts";
import { foundHamlet } from "../realm.ts";
import {
    type AgentRef,
    agentKey,
    nameOf,
    RecordDoc,
    type RecordRefusal,
    recordRefusalText,
    seat as seatRecord,
    type Vote,
    VotesDoc,
} from "../record.ts";
import { ok, type Result } from "../result.ts";
import { answerText, ballotText, summary, tally } from "./words.ts";

type Step = { phase: "hear" } | { phase: "poll" } | { phase: "settle" };
/** The vote runner and its replay-stable errand identities. */
export const RunDoc = defineDocFamily<
    { runner: TaskId<string> | null; errands: Record<string, ConversationId> },
    string
>({
    kind: "hamlet.vote-run",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ runner: null, errands: {} }),
});
/** A ballot or hearing's corpus, deadline and request count. */
export const PollDoc = defineDoc<{
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
/** The durable hearing, poll and settlement phases of a vote. */
export function voteTask(home: Home) {
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
                                    ? clip(answer.text.trim(), 600, {
                                          flat: false,
                                          dots: false,
                                      })
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
                                    reason = `could not vote: ${answer.reason}`;
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
                                await tellChiefs(
                                    home,
                                    tx,
                                    hamlet,
                                    `Vote ${id}: ${nameOf(await tx.doc(RecordDoc, hamlet, hamlet), number)} did not answer. ${reason}`,
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
                            if (
                                home.workspace !== null &&
                                vote.motion.kind === "found_hamlet"
                            )
                                failed =
                                    "founding another hamlet is not supported by remote workspaces";
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
                                    else {
                                        const seated = await seat(home, tx, {
                                            hamlet,
                                            number: vote.motion.candidate,
                                            by: `vote:${id}`,
                                            reason: vote.reason,
                                        });
                                        if (!seated.ok)
                                            failed = `it passed but could not be done: ${recordRefusalText(seated.error, "chief")}`;
                                    }
                                } else {
                                    const place = (
                                        await tx.doc(RealmDoc)
                                    ).hamlets.find((h) => h.id === hamlet);
                                    const founded = await foundHamlet(
                                        home,
                                        tx,
                                        {
                                            ...vote.motion,
                                            chiefs: place?.chiefs ?? [],
                                            from: { hamlet, vote: id },
                                        },
                                    );
                                    if (!founded.ok)
                                        failed = `it passed but could not be done: ${recordRefusalText(founded.error, "chief")}`;
                                }
                            }
                            vote.standing = failed ? { failed } : "passed";
                            await tellChiefs(
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
                            await tellChiefs(
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

async function ask(
    home: Home,
    hamlet: string,
    vote: Vote,
    number: number,
    kind: "ballot" | "answer",
    text: string,
    context: Context,
    history: string[] = [],
): Promise<ErrandOutcome> {
    const key = runKey(hamlet, vote.id),
        name = kind === "answer" ? "answer" : String(number);
    const known = (await home.harness.snapshot(RunDoc, key, context))?.errands[
        name
    ];
    const corpus =
        known === undefined
            ? `${await home.houses.of({ hamlet, number }).notesText(context)}\n${history.join("\n")}`
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
    return runErrand(
        home,
        id,
        {
            requestId: `vote:${hamlet}:${vote.id}:${name}`,
            text,
            task: vote.task,
            timeoutMs: Math.max(0, (poll?.until ?? home.now()) - home.now()),
        },
        context,
    );
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
/** Waits for the recorded runner and renders its current count. */
export async function finish(
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
/** The runner family key within a hamlet. */
export function runKey(hamlet: string, id: number): string {
    return `${hamlet}:${id}`;
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
                        text: `- ${who}: ${clip(text.trim(), 320, { flat: false, dots: false })}`,
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

/** Changes the office, reconfigures conversations and tells the chiefs atomically. */
export async function seat(
    home: Home,
    tx: Tx,
    given: { hamlet: string; number: number; by: string; reason: string },
): Promise<Result<void, RecordRefusal>> {
    const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
    const from = r.mayor;
    const result = await seatRecord(tx, given.hamlet, given.number, {
        ...given,
        at: home.now(),
    });
    if (!result.ok) return result;
    await reconfigure(home, tx);
    await tellChiefs(
        home,
        tx,
        given.hamlet,
        `\`${nameOf(r, given.number)}\` became mayor in place of \`${nameOf(r, from)}\`. ${given.reason}`,
    );
    return ok(undefined);
}
