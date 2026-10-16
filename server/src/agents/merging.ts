import { isUtf8 } from "node:buffer";
import { createHash } from "node:crypto";
import {
    lstat,
    mkdir,
    mkdtemp,
    readFile,
    rm,
    writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { Type } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    defineDoc,
    defineExtension,
    defineTool,
    GenerationTask,
    hook,
    ToolTask,
    type Tx,
} from "@earendil-works/pi-durable";
import { WithDoc } from "../chats/conversations.ts";
import { ErrandDoc, runErrand, startErrand } from "../chats/errands.ts";
import { InstructionsDoc, owed } from "../chats/instructions.ts";
import { newTask, turnOf } from "../chats/turn.ts";
import { stopAgent } from "../chats.ts";
import { houseReader, treeWorker } from "../environment.ts";
import { revoke } from "../gate.ts";
import { untilAborted } from "../home/abort.ts";
import { houseOf } from "../home/layout.ts";
import type { Deed, Home, Part } from "../home.ts";
import {
    list as listTree,
    MERGE_CAPS,
    remove as removeTree,
    stage as stageTree,
    treeFailureText,
} from "../house.ts";
import { LimitsDoc } from "../limits/rules.ts";
import { lapseAgent } from "../oversight/waits.ts";
import { chiefAccess, chiefRefusalText } from "../realm/access.ts";
import { isChief, liveHamlets, RealmDoc } from "../realm/places.ts";
import { farewell, roadsOf } from "../realm/roads.ts";
import {
    type AgentRecord,
    type AgentRef,
    type Archived,
    agentKey,
    checkName,
    named,
    RecordDoc,
} from "../record.ts";
import type { SandboxEnv } from "../sandbox.ts";
import { safeWords } from "../scrub.ts";
import { booksOf } from "../spend/ledger.ts";
import { stopped } from "../spend.ts";
import { decisionText, mayDecide, notifyChiefs, voteOpen } from "../votes.ts";
import { lapseTrust } from "./leaving.ts";

type Planned = {
    files: Map<string, Buffer>;
    clashes: string[];
    before: string;
    after: string;
    notes: "joined" | "rewritten" | "plain";
};
type NotesFailure = {
    kind: "notes-unreadable";
    operation: "merge" | "split";
    agent: AgentRef;
    reason: "not-utf8" | "unavailable";
    retrySafe: true;
};
type PrepareFailure =
    | {
          kind: "destination-blocked";
          operation: "merge";
          path: string;
          retrySafe: true;
      }
    | NotesFailure;
type Prepared =
    | { ok: true; value: Planned }
    | { ok: false; error: PrepareFailure };

function prepareText(error: PrepareFailure): string {
    switch (error.kind) {
        case "notes-unreadable":
            return notesFailureText(error);
        case "destination-blocked":
            return `The merge cannot safely deliver \`${error.path}\`; keep its destination and any linked parent aside before merging.`;
    }
    const unhandled: never = error;
    throw new Error(`Unclassified merge preparation failure: ${unhandled}`);
}

type Given = {
    hamlet: string;
    keep: string;
    absorb: string;
    purpose: string | null;
    reason: string;
    by: string;
};

const MergeCallDoc = defineDoc<{
    errand: ConversationId | null;
    archive: string | null;
    text: string;
}>({
    kind: "hamlet.merge-call",
    version: 1,
    scope: "task",
    initial: () => ({ errand: null, archive: null, text: "" }),
});

const HeldDoc = defineDoc<{ agents: string[] }>({
    kind: "hamlet.merge-held",
    version: 1,
    scope: "session",
    initial: () => ({ agents: [] }),
});

async function stoppedHouses<T>(
    home: Home,
    hamlet: string,
    agents: AgentRecord[],
    context: Context,
    action: () => Promise<T>,
): Promise<T> {
    const added = await home.harness.commit(async (tx) => {
        const held = await tx.doc(HeldDoc);
        const added = agents
            .map((a) => agentKey({ hamlet, number: a.number }))
            .filter((key) => !held.agents.includes(key));
        held.agents.push(...added);
        return added;
    }, context);
    try {
        for (const agent of agents)
            await stopAgent(home, { hamlet, number: agent.number }, context);
        return await action();
    } finally {
        await home.harness.commit(async (tx) => {
            const held = await tx.doc(HeldDoc);
            held.agents = held.agents.filter((key) => !added.includes(key));
        }, BACKGROUND_CONTEXT);
    }
}

const operations = new WeakMap<Home, Promise<unknown>>();

/** Serializes merge, split and erasure file work within one home. */
export async function serialHouseWork<T>(
    home: Home,
    context: Context,
    action: () => Promise<T>,
): Promise<T> {
    const run = (operations.get(home) ?? Promise.resolve())
        .catch(() => {})
        .then(() => {
            context.abortSignal?.throwIfAborted();
            return action();
        });
    operations.set(home, run);
    try {
        return await run;
    } finally {
        if (operations.get(home) === run) operations.delete(home);
    }
}

/** Merges and their reversal. */
export const Merging: Part = (home) => ({
    extension: defineExtension({
        name: "hamlet.merging",
        hooks: [
            hook(GenerationTask, {
                beforeRequest: async (_request, api, context) => {
                    if (
                        (
                            await api.snapshot(
                                ErrandDoc,
                                api.conversationId,
                                context,
                            )
                        )?.kind === "ballot"
                    )
                        return undefined;
                    const place = await api.snapshot(
                        WithDoc,
                        api.conversationId,
                        context,
                    );
                    if (
                        place &&
                        (await api.snapshot(HeldDoc, context))?.agents.includes(
                            agentKey(place),
                        )
                    ) {
                        void home.harness.abortTask(
                            api.taskId,
                            BACKGROUND_CONTEXT,
                        );
                        return untilAborted(context);
                    }
                },
            }),
            hook(ToolTask, {
                beforeTool: async (_call, api, context) => {
                    if (
                        (
                            await api.snapshot(
                                ErrandDoc,
                                api.conversationId,
                                context,
                            )
                        )?.kind === "ballot"
                    )
                        return undefined;
                    const place = await api.snapshot(
                        WithDoc,
                        api.conversationId,
                        context,
                    );
                    if (
                        place &&
                        (await api.snapshot(HeldDoc, context))?.agents.includes(
                            agentKey(place),
                        )
                    )
                        return {
                            block: "This house is being merged or split. Wait for its files to finish, then ask the chief to send a new instruction.",
                        };
                    return undefined;
                },
            }),
        ],
        tools: [
            defineTool({
                name: "merge_agents",
                description:
                    "Merge two agents' houses and roads under the kept agent's new purpose. A reason is kept in the record.",
                parameters: Type.Object({
                    keep: Type.String(),
                    absorb: Type.String(),
                    purpose: Type.String(),
                    reason: Type.String(),
                }),
                replay: "safe",
                execute: async (args, api, context) =>
                    serialHouseWork(home, context, async () => {
                        const place = await api.snapshot(
                            WithDoc,
                            api.conversationId,
                            context,
                        );
                        if (!place)
                            throw new Error("This conversation has no agent.");
                        const saved = await api.snapshot(
                            MergeCallDoc,
                            api.taskId,
                            context,
                        );
                        if (saved?.archive) {
                            const entry = (
                                await api.snapshot(
                                    RecordDoc,
                                    place.hamlet,
                                    context,
                                )
                            )?.archive.find((a) => a.id === saved.archive);
                            if (entry) {
                                const outcome = await retryFiles(
                                    home,
                                    place.hamlet,
                                    entry,
                                    context,
                                );
                                if (!outcome.ok)
                                    return {
                                        output: [
                                            {
                                                type: "text",
                                                text: pendingText(
                                                    outcome.error,
                                                ),
                                            },
                                        ],
                                    };
                            }
                            return {
                                output: [{ type: "text", text: saved.text }],
                            };
                        }
                        const wait = await voteOpen(
                            api,
                            place.hamlet,
                            home.now(),
                            context,
                        );
                        if (wait)
                            return { output: [{ type: "text", text: wait }] };
                        const given: Given = {
                            ...args,
                            hamlet: place.hamlet,
                            by: "mayor",
                        };
                        const agents = await api.commit(async (tx) => {
                            const r = await tx.doc(
                                RecordDoc,
                                place.hamlet,
                                place.hamlet,
                            );
                            if (
                                r.mayor !== place.number ||
                                !r.agents.some((a) => a.number === place.number)
                            )
                                throw new Error(
                                    "Only the mayor can merge agents.",
                                );
                            return allowed(home, tx, given);
                        }, context);
                        return stoppedHouses(
                            home,
                            place.hamlet,
                            [agents.kept, agents.gone],
                            context,
                            async () => {
                                const prepared = await prepare(
                                    home,
                                    place.hamlet,
                                    agents,
                                    context,
                                );
                                if (!prepared.ok)
                                    return {
                                        output: [
                                            {
                                                type: "text",
                                                text: prepareText(
                                                    prepared.error,
                                                ),
                                            },
                                        ],
                                    };
                                const plan = prepared.value;
                                let why = "";
                                if (plan.notes === "joined") {
                                    const limits = await api.snapshot(
                                        LimitsDoc,
                                        place.hamlet,
                                        context,
                                    );
                                    if (!limits)
                                        throw new Error(
                                            "The hamlet has no limits.",
                                        );
                                    const turn = await turnOf(
                                        home.harness,
                                        api,
                                        context,
                                    );
                                    const task =
                                        turn?.task ?? newTask(home.now());
                                    why =
                                        stopped(
                                            limits,
                                            await booksOf(
                                                home,
                                                place.hamlet,
                                                limits.budget.period,
                                                task,
                                                context,
                                            ),
                                        ) ?? "";
                                    if (!why) {
                                        const errand = await api.commit(
                                            async (tx) => {
                                                const call = await tx.doc(
                                                    MergeCallDoc,
                                                    api.taskId,
                                                );
                                                call.errand ??=
                                                    await startErrand(
                                                        home,
                                                        tx,
                                                        {
                                                            agent: place,
                                                            kind: "notes",
                                                            own: false,
                                                            instructions:
                                                                mergeRequest(
                                                                    agents.kept,
                                                                    agents.gone,
                                                                    args.purpose,
                                                                    limits
                                                                        .leaving
                                                                        .mergedNotes,
                                                                ),
                                                            tools: [],
                                                        },
                                                    );
                                                return call.errand;
                                            },
                                            context,
                                        );
                                        const answer = await runErrand(
                                            home,
                                            errand,
                                            {
                                                requestId: `merge-notes:${api.taskId}`,
                                                text: `Your NOTES.md now:\n<notes of="${args.keep}">\n${plan.before}\n</notes>\n\nThe NOTES.md of \`${args.absorb}\`:\n<notes of="${args.absorb}">\n${plan.files.get(`merged/${args.absorb}/NOTES.md`)?.toString("utf8") ?? ""}\n</notes>`,
                                                task,
                                            },
                                            context,
                                        );
                                        const rewritten =
                                            answer.status === "done"
                                                ? accept(
                                                      answer.text,
                                                      limits.leaving
                                                          .mergedNotes,
                                                  )
                                                : undefined;
                                        if (rewritten) {
                                            plan.after = `${rewritten}\n\n${mergedParagraph(agents.gone)}`;
                                            plan.notes = "rewritten";
                                        } else
                                            why =
                                                answer.status === "done"
                                                    ? "The reply could not be used as merged notes."
                                                    : answer.text;
                                    }
                                }
                                const committed = await api.commit(
                                    async (tx) => {
                                        const decision = await mayDecide(
                                            tx,
                                            place.hamlet,
                                            home.now(),
                                            "merge",
                                        );
                                        if (!decision.ok) return decision;
                                        const call = await tx.doc(
                                            MergeCallDoc,
                                            api.taskId,
                                        );
                                        const r = await tx.doc(
                                            RecordDoc,
                                            place.hamlet,
                                            place.hamlet,
                                        );
                                        if (r.mayor !== place.number)
                                            throw new Error(
                                                "Only the mayor can merge agents.",
                                            );
                                        const entry = await archive(
                                            home,
                                            tx,
                                            given,
                                            agents,
                                            plan,
                                        );
                                        call.archive = entry.id;
                                        call.text =
                                            report(
                                                entry,
                                                plan.notes === "joined",
                                                args.purpose,
                                            ) + (why ? ` ${why}` : "");
                                        return {
                                            ok: true,
                                            value: entry,
                                        } satisfies {
                                            ok: true;
                                            value: Archived;
                                        };
                                    },
                                    context,
                                );
                                if (!committed.ok)
                                    return {
                                        output: [
                                            {
                                                type: "text",
                                                text: decisionText(
                                                    committed.error,
                                                ),
                                            },
                                        ],
                                    };
                                const entry = committed.value;
                                const outcome = await retryFiles(
                                    home,
                                    place.hamlet,
                                    entry,
                                    context,
                                );
                                if (!outcome.ok)
                                    return {
                                        output: [
                                            {
                                                type: "text",
                                                text: pendingText(
                                                    outcome.error,
                                                ),
                                            },
                                        ],
                                    };
                                const result = await api.snapshot(
                                    MergeCallDoc,
                                    api.taskId,
                                    context,
                                );
                                return {
                                    output: [
                                        {
                                            type: "text",
                                            text:
                                                result?.text ??
                                                report(
                                                    entry,
                                                    plan.notes === "joined",
                                                    args.purpose,
                                                ),
                                        },
                                    ],
                                };
                            },
                        );
                    }),
            }),
        ],
    }),
    async start(context) {
        await home.harness.commit(async (tx) => {
            (await tx.doc(HeldDoc)).agents = [];
        }, context);
        const realm = await home.harness.snapshot(RealmDoc, context);
        for (const { id } of liveHamlets(realm)) {
            const record = await home.harness.snapshot(RecordDoc, id, context);
            for (const entry of record?.archive ?? [])
                if (entry.merge && !entry.merge.applied)
                    await retryFiles(home, id, entry, context);
        }
    },
});

/** A chief's merge deed. */
export const merge: Deed = async (home, given, context) =>
    serialHouseWork(home, context, async () => {
        const [absorb = "", into, keep = "", ...reason] = given.rest
            .trim()
            .split(/\s+/);
        if (into !== "into")
            throw new Error("Use /merge <absorbed> into <kept> [reason].");
        const args: Given = {
            hamlet: given.hamlet,
            keep,
            absorb,
            purpose: null,
            reason: reason.join(" ") || "a chief asked",
            by: `chief:${given.chief}`,
        };
        const access = await chiefAccess(
            home,
            given.hamlet,
            "merge",
            context,
            given.chief,
        );
        if (!access.ok) return chiefRefusalText(access);
        const record = await home.harness.snapshot(
            RecordDoc,
            given.hamlet,
            context,
        );
        const pending = record?.archive.findLast(
            (entry) =>
                entry.back === null &&
                entry.merge &&
                (entry.id === absorb ||
                    (entry.agent.name === absorb &&
                        record &&
                        !named(record, absorb))),
        );
        if (
            record &&
            pending?.merge &&
            named(record, keep)?.number === pending.merge.kept.number
        ) {
            const outcome = await retryFiles(
                home,
                given.hamlet,
                pending,
                context,
            );
            if (!outcome.ok) return pendingText(outcome.error);
            const kept = record.agents.find(
                (agent) => agent.number === pending.merge?.kept.number,
            );
            return report(
                pending,
                false,
                kept?.purpose ?? pending.merge.kept.purpose,
            );
        }
        const agents = await home.harness.commit(
            (tx) => allowed(home, tx, args),
            context,
        );
        return stoppedHouses(
            home,
            args.hamlet,
            [agents.kept, agents.gone],
            context,
            async () => {
                const prepared = await prepare(
                    home,
                    args.hamlet,
                    agents,
                    context,
                );
                if (!prepared.ok) return prepareText(prepared.error);
                const plan = prepared.value;
                const entry = await home.harness.commit(
                    (tx) => archive(home, tx, args, agents, plan),
                    context,
                );
                const outcome = await retryFiles(
                    home,
                    args.hamlet,
                    entry,
                    context,
                );
                if (!outcome.ok) return pendingText(outcome.error);
                return report(
                    entry,
                    plan.notes === "joined",
                    agents.kept.purpose,
                );
            },
        );
    });

async function allowed(
    home: Home,
    tx: Tx,
    given: Given,
): Promise<{ kept: AgentRecord; gone: AgentRecord }> {
    const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
    if (
        given.by.startsWith("chief:") &&
        !isChief(await tx.doc(RealmDoc), given.hamlet, given.by.slice(6))
    )
        throw new Error("Only a chief of this hamlet can merge agents.");
    if (given.keep === given.absorb)
        throw new Error("an agent cannot be merged with itself");
    const living = (name: string) => {
        const agent = named(r, name);
        if (name === "mayor" || agent?.number === r.mayor)
            throw new Error(
                "the mayor cannot be removed or merged; replacing a mayor takes a chief or a vote",
            );
        if (!agent) {
            const gone = r.archive.findLast(
                (a) => a.back === null && a.agent.name === name,
            );
            throw new Error(
                gone ? farewell(gone, r) : `no agent named \`${name}\``,
            );
        }
        return agent;
    };
    const kept = living(given.keep),
        gone = living(given.absorb);
    if (given.purpose !== null && !given.purpose.trim())
        throw new Error("say what the merged agent is for in `purpose`");
    if (!given.reason.trim())
        throw new Error("give a `reason`; it is kept in the record");
    if (given.by === "mayor") {
        const limits = await tx.doc(LimitsDoc, given.hamlet, given.hamlet);
        for (const agent of [kept, gone]) {
            const key = agentKey({
                hamlet: given.hamlet,
                number: agent.number,
            });
            const i = owed(
                await tx.doc(InstructionsDoc, key, key),
                home.now(),
                limits.leaving.protectedMinutes * 60_000,
            );
            if (i)
                throw new Error(
                    `\`${agent.name}\` has a task from the chief ${i.chief} ("${Array.from(i.text).slice(0, 80).join("")}") that ${i.done === null ? "is not finished" : "has only just ended"}. No agent may be removed or merged to avoid a chief's task. Wait until the chief has had the answer, or ask the chief to do this.`,
                );
        }
    }
    return { kept: { ...kept }, gone: { ...gone } };
}

function envFor(home: Home, ref: AgentRef, write: boolean): SandboxEnv {
    return write
        ? treeWorker(home, {
              root: houseOf(home.layout, ref),
              stage: houseOf(home.layout, ref),
              caps: MERGE_CAPS,
          })
        : houseReader(home, ref, BACKGROUND_CONTEXT);
}

async function notes(
    home: Home,
    ref: AgentRef,
    context: Context,
    operation: "merge" | "split" = "merge",
): Promise<
    | { ok: true; value: { text: string; bytes: Buffer } }
    | { ok: false; error: NotesFailure }
> {
    const env = envFor(home, ref, false);
    try {
        const read = await env.readBinaryFile(
            join(houseOf(home.layout, ref), "NOTES.md"),
            context,
        );
        if (!read.ok) {
            if (read.error.code === "not_found")
                return {
                    ok: true,
                    value: {
                        text: "# Notes\n",
                        bytes: Buffer.from("# Notes\n"),
                    },
                };
            return {
                ok: false,
                error: {
                    kind: "notes-unreadable",
                    operation,
                    agent: ref,
                    reason: "unavailable",
                    retrySafe: true,
                },
            };
        }
        if (!isUtf8(read.value))
            return {
                ok: false,
                error: {
                    kind: "notes-unreadable",
                    operation,
                    agent: ref,
                    reason: "not-utf8",
                    retrySafe: true,
                },
            };
        return {
            ok: true,
            value: {
                text: Buffer.from(read.value).toString("utf8"),
                bytes: Buffer.from(read.value),
            },
        };
    } finally {
        await env.cleanup(context);
    }
}

function substance(text: string): string {
    return text
        .split("\n")
        .filter((line) => line.trim() !== "# Notes")
        .join("\n")
        .trim();
}
function digest(bytes: Buffer | string): string {
    return createHash("sha256").update(bytes).digest("hex");
}

async function prepare(
    home: Home,
    hamlet: string,
    { kept, gone }: { kept: AgentRecord; gone: AgentRecord },
    context: Context,
): Promise<Prepared> {
    const source = houseOf(home.layout, { hamlet, number: gone.number });
    const target = houseOf(home.layout, { hamlet, number: kept.number });
    const stage = await mkdtemp(join(tmpdir(), "hamlet-merge-"));
    try {
        const staged = await stageTree(
            home,
            { root: source, rel: "." },
            stage,
            MERGE_CAPS,
            context,
        );
        if (!staged.ok) {
            switch (staged.error.kind) {
                case "entries-cap":
                case "bytes-cap":
                    throw new Error(
                        "The absorbed house is over 512 MiB or 20000 files. Remove the agent instead.",
                    );
                case "unsafe-entry":
                case "stage-not-empty":
                case "stage-overlaps-source":
                case "path-invalid":
                case "clobber":
                case "shelf-linked":
                case "refused":
                    throw new Error(treeFailureText(staged.error));
            }
        }
        const from = await listTree(home, stage, ".", context);
        if (!from.ok) throw new Error(treeFailureText(from.error));
        const to = await listTree(home, target, ".", context);
        if (!to.ok) throw new Error(treeFailureText(to.error));
        const present = new Map(to.value.files.map((f) => [f.path, f.sha256]));
        const files = new Map<string, Buffer>();
        const clashes: string[] = [];
        const put = (rel: string, bytes: Buffer) => {
            if (
                rel.startsWith("/") ||
                rel.includes("\\") ||
                rel.includes("\0") ||
                rel.split("/").some((p) => !p || p === "." || p === "..")
            )
                throw new Error("A merge needs ordinary relative file paths.");
            if (files.has(rel) || present.has(rel))
                throw new Error(
                    `The merge needs \`${rel}\`, which already exists; keep it aside before merging.`,
                );
            files.set(rel, bytes);
        };
        const keptNotes = await notes(
            home,
            { hamlet, number: kept.number },
            context,
        );
        if (!keptNotes.ok) return keptNotes;
        const before = keptNotes.value.text;
        const theirBytes = from.value.files.some((f) => f.path === "NOTES.md")
            ? await readFile(join(stage, "NOTES.md"))
            : Buffer.from("# Notes\n");
        const theirs = theirBytes.toString("utf8");
        const both = !!substance(before) && !!substance(theirs);
        const body = substance(theirs)
            ? `${before.trimEnd()}\n\n## From ${gone.name}\n${substance(theirs)}`
            : before.trimEnd();
        const after = `${body.trimEnd()}\n\n${mergedParagraph(gone)}`;
        put(`merged/${gone.name}/NOTES.before.md`, keptNotes.value.bytes);
        put(`merged/${gone.name}/NOTES.md`, theirBytes);
        const renamed = new Map<string, string>();
        for (const f of from.value.files) {
            const skill = /^skills\/([^/]+)\//.exec(f.path)?.[1];
            if (!skill || renamed.has(skill)) continue;
            const prefix = `skills/${skill}/`;
            const theirs = from.value.files.filter((f) =>
                f.path.startsWith(prefix),
            );
            const mine = to.value.files.filter((f) =>
                f.path.startsWith(prefix),
            );
            const same =
                theirs.length === mine.length &&
                theirs.every((f) => present.get(f.path) === f.sha256);
            if (same) renamed.set(skill, "");
            else if (mine.length > 0) {
                const name = `${skill}-from-${gone.name}`;
                renamed.set(skill, name);
                clashes.push(
                    `skill \`${skill}\` (the absorbed one is \`${name}\`)`,
                );
            } else renamed.set(skill, skill);
        }
        for (const f of from.value.files) {
            if (f.path === "NOTES.md") continue;
            let rel = f.path;
            let bytes: Buffer = await readFile(join(stage, f.path));
            const skill = /^skills\/([^/]+)\//.exec(rel)?.[1];
            if (skill) {
                const name = renamed.get(skill);
                if (!name) continue;
                if (name !== skill) {
                    rel = rel.replace(`skills/${skill}/`, `skills/${name}/`);
                    bytes = renamedSkill(bytes, skill, name);
                }
            } else if (present.has(rel)) {
                if (present.get(rel) === f.sha256) continue;
                rel = `merged/${gone.name}/${rel}`;
                clashes.push(
                    `file \`${f.path}\` (the absorbed one is \`${rel}\`)`,
                );
            }
            put(rel, bytes);
        }
        for (const rel of files.keys()) {
            const parts = rel.split("/");
            let path = target;
            for (const [index, part] of parts.entries()) {
                path = join(path, part);
                const stat = await lstat(path).catch((error: unknown) => {
                    if (
                        error instanceof Error &&
                        "code" in error &&
                        error.code === "ENOENT"
                    )
                        return undefined;
                    throw error;
                });
                if (stat && (index === parts.length - 1 || !stat.isDirectory()))
                    return {
                        ok: false,
                        error: {
                            kind: "destination-blocked",
                            operation: "merge",
                            path: rel,
                            retrySafe: true,
                        },
                    };
            }
        }
        return {
            ok: true,
            value: {
                files,
                clashes,
                before,
                after,
                notes: both ? "joined" : "plain",
            },
        };
    } finally {
        await rm(stage, { recursive: true, force: true });
    }
}

function renamedSkill(bytes: Buffer, old: string, name: string): Buffer {
    if (!isUtf8(bytes)) return bytes;
    let named = false;
    return Buffer.from(
        bytes
            .toString("utf8")
            .replaceAll(`skills/${old}/`, `skills/${name}/`)
            .split("\n")
            .map((line) => {
                const colon = line.indexOf(":");
                if (
                    !named &&
                    line.slice(0, colon).trim() === "name" &&
                    line
                        .slice(colon + 1)
                        .trim()
                        .replace(/^"|"$/g, "") === old
                ) {
                    named = true;
                    return `name: ${name}`;
                }
                return line;
            })
            .join("\n"),
    );
}

async function broughtBytes(
    source: string,
    entry: Archived,
    f: { path: string; sha256: string },
): Promise<Buffer> {
    const aside = `merged/${entry.agent.name}/`;
    if (f.path === `${aside}NOTES.before.md`)
        return Buffer.from(entry.merge?.notesBefore ?? "");
    const rel = f.path.startsWith(aside) ? f.path.slice(aside.length) : f.path;
    const candidates = [f.path, rel];
    for (const path of candidates) {
        const bytes = await readFile(join(source, path)).catch(() => undefined);
        if (bytes && digest(bytes) === f.sha256) return bytes;
    }
    const name = /^skills\/([^/]+)\//.exec(rel)?.[1];
    const suffix = `-from-${entry.agent.name}`;
    if (name?.endsWith(suffix)) {
        const old = name.slice(0, -suffix.length);
        const path = rel.replace(`skills/${name}/`, `skills/${old}/`);
        const bytes = await readFile(join(source, path)).catch(() => undefined);
        if (bytes) {
            const changed = renamedSkill(bytes, old, name);
            if (digest(changed) === f.sha256) return changed;
        }
    }
    if (rel === "NOTES.md" && digest("# Notes\n") === f.sha256)
        return Buffer.from("# Notes\n");
    throw new Error(
        "The archived house changed before its merge could finish.",
    );
}

async function archive(
    home: Home,
    tx: Tx,
    given: Given,
    was: { kept: AgentRecord; gone: AgentRecord },
    plan: Planned,
): Promise<Archived> {
    const { kept, gone } = await allowed(home, tx, given);
    if (kept.number !== was.kept.number || gone.number !== was.gone.number)
        throw new Error(
            "One of the agents left while this was decided; nothing was merged.",
        );
    const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
    const at = home.now();
    let stamp = at;
    while (r.archive.some((a) => a.id === `${gone.name}-${stamp}`)) stamp++;
    const roads = roadsOf(r).filter(
        (r) => r.a === gone.number || r.b === gone.number,
    );
    const inherited: NonNullable<Archived["merge"]>["inherited"] = [];
    await lapseAgent(tx, { hamlet: given.hamlet, number: gone.number }, at);
    await lapseTrust(tx, { hamlet: given.hamlet, number: gone.number });
    for (const p of ["network", "unconfined"] as const)
        await revoke(
            tx,
            { hamlet: given.hamlet, number: gone.number },
            p,
            given.by,
            at,
        );
    r.roads = r.roads.filter((r) => r.a !== gone.number && r.b !== gone.number);
    for (const road of roads) {
        const peer = road.a === gone.number ? road.b : road.a;
        if (peer === kept.number) continue;
        const existing = r.roads.find(
            (r) =>
                (r.a === kept.number && r.b === peer) ||
                (r.b === kept.number && r.a === peer),
        );
        const fresh = !existing;
        const lastFrom =
            road.lastFrom === gone.number ? kept.number : road.lastFrom;
        if (!existing)
            r.roads.push({ ...road, a: kept.number, b: peer, lastFrom });
        else {
            existing.messages += road.messages;
            if (road.lastUsed > existing.lastUsed) {
                existing.lastUsed = road.lastUsed;
                existing.lastFrom = lastFrom;
            }
        }
        inherited.push({ peer, messages: road.messages, fresh });
    }
    const entry: Archived = {
        id: `${gone.name}-${stamp}`,
        agent: gone,
        how: "merged",
        into: kept.number,
        at,
        by: given.by,
        reason: given.reason.trim(),
        roads,
        merge: {
            kept,
            notesBefore: plan.before,
            notesAfter: plan.after,
            brought: [...plan.files].map(([path, bytes]) => ({
                path,
                sha256: digest(bytes),
            })),
            clashes: plan.clashes,
            inherited,
            applied: false,
        },
        back: null,
    };
    r.agents = r.agents.filter((a) => a.number !== gone.number);
    const living = r.agents.find((a) => a.number === kept.number);
    if (living) living.purpose = given.purpose?.trim() ?? kept.purpose;
    r.archive.push(entry);
    r.decisions.push({
        kind: "merged",
        at,
        by: given.by,
        reason: entry.reason,
        keep: kept.number,
        absorb: gone.number,
        purpose: living?.purpose ?? kept.purpose,
        notes: plan.notes,
    });
    return JSON.parse(JSON.stringify(entry)) as Archived;
}

async function apply(
    home: Home,
    hamlet: string,
    entry: Archived,
    context: Context,
): Promise<FileResult> {
    const merge = entry.merge;
    if (!merge || merge.applied) return { ok: true };
    await stopAgent(home, { hamlet, number: entry.agent.number }, context);
    await stopAgent(home, { hamlet, number: merge.kept.number }, context);
    const source = await mkdtemp(join(tmpdir(), "hamlet-merge-source-"));
    const delivery = await mkdtemp(join(tmpdir(), "hamlet-merge-delivery-"));
    const target = houseOf(home.layout, { hamlet, number: merge.kept.number });
    try {
        const treeOutcome2 = await stageTree(
            home,
            {
                root: houseOf(home.layout, {
                    hamlet,
                    number: entry.agent.number,
                }),
                rel: ".",
            },
            source,
            MERGE_CAPS,
            context,
        );
        if (!treeOutcome2.ok)
            throw new Error(treeFailureText(treeOutcome2.error));
        const listed = await listTree(home, target, ".", context);
        if (!listed.ok) throw new Error(treeFailureText(listed.error));
        const present = new Map(
            listed.value.files.map((f) => [f.path, f.sha256]),
        );
        const pending: string[] = [];
        for (const f of merge.brought) {
            if (present.get(f.path) === f.sha256) continue;
            if (present.has(f.path))
                throw new Error(
                    `The pending merge cannot replace changed file \`${f.path}\`.`,
                );
            const bytes = await broughtBytes(source, entry, f);
            const staged = join(delivery, "tree", f.path);
            await mkdir(dirname(staged), { recursive: true });
            await writeFile(staged, bytes);
            pending.push(f.path);
        }
        if (pending.length) {
            await writeFile(
                join(delivery, "manifest.json"),
                JSON.stringify(pending),
            );
            const copier = treeWorker(home, {
                root: delivery,
                stage: target,
                caps: MERGE_CAPS,
            });
            try {
                const copied = await copier.exec(
                    [
                        process.execPath,
                        "-e",
                        String.raw`
const fs = require("node:fs"), p = require("node:path");
const [root, stage] = process.argv.slice(1);
for (const rel of JSON.parse(fs.readFileSync(p.join(stage, "manifest.json"), "utf8"))) {
    const parts = rel.split("/");
    if (!rel || rel.includes("\\") || parts.some(s => !s || s === "." || s === "..")) throw Error("relative path required");
    let parent = root;
    for (const part of parts.slice(0, -1)) {
        parent = p.join(parent, part);
        try { fs.mkdirSync(parent); } catch (e) { if (e.code !== "EEXIST") throw e; }
        if (!fs.lstatSync(parent).isDirectory()) throw Error("real parent required");
    }
    fs.copyFileSync(p.join(stage, "tree", rel), p.join(root, rel), fs.constants.COPYFILE_EXCL);
}`,
                        target,
                        delivery,
                    ],
                    undefined,
                    context,
                );
                if (!copied.ok || copied.value.exitCode !== 0)
                    return {
                        ok: false,
                        error: {
                            kind: "pending-files",
                            operation: "merge",
                            hamlet,
                            archive: entry.id,
                            kept: merge.kept.name,
                            absorbed: entry.agent.name,
                            retrySafe: true,
                        },
                    };
            } finally {
                await copier.cleanup(context);
            }
        }
        const env = envFor(home, { hamlet, number: merge.kept.number }, true);
        try {
            const read = await notes(
                home,
                { hamlet, number: merge.kept.number },
                context,
            );
            if (!read.ok) return read;
            const current = read.value.text;
            if (current !== merge.notesBefore && current !== merge.notesAfter)
                return {
                    ok: false,
                    error: {
                        kind: "changed-notes",
                        operation: "merge",
                        hamlet,
                        archive: entry.id,
                        kept: merge.kept.name,
                        absorbed: entry.agent.name,
                        retrySafe: true,
                    },
                };
            const result = await env.writeFile(
                join(target, "NOTES.md"),
                merge.notesAfter,
                context,
            );
            if (!result.ok)
                throw new Error(
                    "The merged notes could not be written safely.",
                );
        } finally {
            await env.cleanup(context);
        }
        await home.harness.commit(async (tx) => {
            const a = (await tx.doc(RecordDoc, hamlet, hamlet)).archive.find(
                (a) => a.id === entry.id,
            );
            if (a?.merge) a.merge.applied = true;
        }, context);
    } finally {
        await rm(source, { recursive: true, force: true });
        await rm(delivery, { recursive: true, force: true });
    }
    return { ok: true };
}

function mergedParagraph(gone: AgentRecord): string {
    return `## Merged\n- \`${gone.name}\` was merged into me. Its purpose was: ${gone.purpose} Its notes as they were are in merged/${gone.name}/NOTES.md and mine in merged/${gone.name}/NOTES.before.md. Files of its that differed from mine of the same name are under merged/${gone.name}/.\n`;
}

function mergeRequest(
    kept: AgentRecord,
    gone: AgentRecord,
    purpose: string,
    limit: number,
): string {
    return `You are \`${kept.name}\`, an agent in a hamlet of agents. The mayor is merging the agent \`${gone.name}\` into you: from now on you do the work of both, under your own name. Your purpose was: ${kept.purpose} Its purpose was: ${gone.purpose} Your purpose becomes: ${purpose.trim()}\nYou are writing your new NOTES.md, the memory that is shown to you on every turn.\nWrite the one NOTES.md you will keep.\n- Keep every fact, preference and lesson from both. Drop nothing because it looks minor.\n- Where both say the same thing, say it once.\n- Where they disagree, keep both and say which of you held which.\n- Speak as yourself: things \`${gone.name}\` wrote as "I" are now things you know or do.\n- Group it by subject, not by who wrote it.\n- At most ${limit} characters.\n- Start with the line \`# Notes\`. Reply with the file's text only: no code fence, no comment before or after.`;
}

function accept(reply: string, limit: number): string | undefined {
    let text = reply.trim();
    if (text.includes("</think>"))
        text = text.slice(text.lastIndexOf("</think>") + 8).trim();
    if (text.startsWith("```"))
        text = text
            .slice(text.indexOf("\n") + 1)
            .replace(/```\s*$/, "")
            .trim();
    if (text.includes("# Notes")) text = text.slice(text.indexOf("# Notes"));
    return substance(text) && Array.from(text).length <= limit
        ? text
        : undefined;
}

function report(entry: Archived, joined: boolean, purpose: string): string {
    const m = entry.merge;
    return `Merged \`${entry.agent.name}\` into \`${m?.kept.name}\`. \`${m?.kept.name}\` has the notes, skills, files and roads of both, and its purpose is now: ${purpose} \`${entry.agent.name}\` is archived as ${entry.id}.${joined ? ` The two notes were joined, not rewritten: ask \`${m?.kept.name}\` to tidy its NOTES.md.` : ""}${m?.clashes.length ? ` Kept apart because they differed: ${m.clashes.join("; ")}.` : ""}`;
}

/** A merged agent split back out of its kept house. */
export async function split(
    home: Home,
    given: {
        hamlet: string;
        entry: Archived;
        name: string;
        reason: string;
        chief: string;
    },
    context: Context,
): Promise<string> {
    return serialHouseWork(home, context, async () => {
        if (given.entry.merge && !given.entry.merge.applied) {
            const outcome = await retryFiles(
                home,
                given.hamlet,
                given.entry,
                context,
            );
            if (!outcome.ok) return pendingText(outcome.error);
            if (given.entry.back !== null)
                return `Split of \`${given.entry.agent.name}\` is finished. Read /record to inspect it.`;
        }
        return splitOnce(home, given, context);
    });
}

async function splitOnce(
    home: Home,
    given: {
        hamlet: string;
        entry: Archived;
        name: string;
        reason: string;
        chief: string;
    },
    context: Context,
): Promise<string> {
    const pending = await home.harness.commit(async (tx) => {
        if (!isChief(await tx.doc(RealmDoc), given.hamlet, given.chief))
            throw new Error("Only a chief of this hamlet can restore agents.");
        const r = await tx.doc(RecordDoc, given.hamlet, given.hamlet);
        const at = r.archive.findIndex(
            (a) => a.id === given.entry.id && a.back === null,
        );
        const entry = r.archive[at];
        if (!entry?.merge)
            throw new Error(
                `nothing is archived under \`${given.entry.id}\`; /record lists what is`,
            );
        if (!entry.merge.applied)
            throw new Error(
                "The merge has not finished yet; try again after it finishes.",
            );
        const kept = r.agents.find(
            (a) => a.number === entry.merge?.kept.number,
        );
        if (!kept)
            throw new Error(
                `\`${entry.merge.kept.name}\`, which \`${entry.agent.name}\` was merged into, has left the hamlet itself; restore that first`,
            );
        const later = r.archive
            .slice(at + 1)
            .find(
                (a) => a.back === null && a.merge?.kept.number === kept.number,
            );
        if (later)
            throw new Error(
                `\`${later.agent.name}\` was merged into \`${kept.name}\` after \`${entry.agent.name}\`; split that one first`,
            );
        if (given.name !== entry.agent.name) checkName(given.name);
        for (const h of liveHamlets(await tx.doc(RealmDoc)))
            if (
                (await tx.doc(RecordDoc, h.id, h.id)).agents.some(
                    (a) => a.name === given.name,
                )
            )
                throw new Error(
                    `an agent named \`${given.name}\` lives in the hamlet now. Restore this one under another name: /restore ${entry.id} as <new-name>`,
                );
        const limits = await tx.doc(LimitsDoc, given.hamlet, given.hamlet);
        if (r.agents.length - 1 >= limits.mostAgents)
            throw new Error(
                `the hamlet is full (${limits.mostAgents} agents); remove one first, or raise mostAgents in the limits`,
            );
        kept.purpose = entry.merge.kept.purpose;
        for (const i of entry.merge.inherited) {
            const index = r.roads.findIndex(
                (road) =>
                    (road.a === kept.number && road.b === i.peer) ||
                    (road.b === kept.number && road.a === i.peer),
            );
            const road = r.roads[index];
            if (!road) continue;
            road.messages = Math.max(0, road.messages - i.messages);
            if (i.fresh && road.messages === 0) r.roads.splice(index, 1);
        }
        r.agents.push({ ...entry.agent, name: given.name });
        for (const road of entry.roads)
            if (
                [road.a, road.b].every(
                    (end) =>
                        end === "office" ||
                        r.agents.some((a) => a.number === end),
                ) &&
                !r.roads.some(
                    (r) =>
                        (r.a === road.a && r.b === road.b) ||
                        (r.a === road.b && r.b === road.a),
                )
            )
                r.roads.push({ ...road });
        entry.back = given.name;
        entry.merge.applied = false;
        r.decisions.push({
            kind: "restored",
            at: home.now(),
            by: `chief:${given.chief}`,
            reason: given.reason,
            agent: entry.agent.number,
            name: given.name,
            archive: entry.id,
        });
        const peers = roadsOf(r)
            .filter(
                (road) =>
                    road.a === entry.agent.number ||
                    road.b === entry.agent.number,
            )
            .map((road) => {
                const end = road.a === entry.agent.number ? road.b : road.a;
                return end === "office"
                    ? "mayor"
                    : (r.agents.find((a) => a.number === end)?.name ?? "");
            });
        const built = roadsOf(r).filter(
            (r) => r.a !== "office" && r.b !== "office",
        ).length;
        return {
            entry: JSON.parse(JSON.stringify(entry)) as Archived,
            words: `Split \`${given.name}\` from \`${kept.name}\`. \`${given.name}\` has its house as it was before the merge, and \`${kept.name}\` its old purpose and whatever it has learned since. \`${given.name}\`'s roads lead to: ${peers.join(", ")}.${built > limits.mostRoads ? ` The hamlet now has ${built} roads between agents, more than the ${limits.mostRoads} your limits allow. They all work; the mayor can build no more until you raise mostRoads in the limits.` : ""}`,
        };
    }, context);
    const outcome = await retryFiles(
        home,
        given.hamlet,
        pending.entry,
        context,
    );
    return outcome.ok ? pending.words : pendingText(outcome.error);
}

async function part(
    home: Home,
    hamlet: string,
    entry: Archived,
    context: Context,
): Promise<FileResult> {
    const merge = entry.merge;
    if (!merge || merge.applied) return { ok: true };
    const ref = { hamlet, number: merge.kept.number };
    await stopAgent(home, ref, context);
    const root = houseOf(home.layout, ref);
    const env = envFor(home, ref, true);
    try {
        const read = await notes(home, ref, context, "split");
        if (!read.ok) return read;
        const current = read.value.text;
        const clean =
            current === merge.notesAfter || current === merge.notesBefore;
        const said = `\`${entry.agent.name}\` was split off again`;
        const text = clean
            ? merge.notesBefore
            : current.includes(said)
              ? current
              : `${current.trimEnd()}\n\n## Split\n- ${said} and has its own house. What these notes hold of its work is its business now. My notes from before the merge are in merged/${entry.agent.name}/NOTES.before.md.\n`;
        const written = await env.writeFile(
            join(root, "NOTES.md"),
            text,
            context,
        );
        if (!written.ok)
            throw new Error("The split notes could not be written safely.");
        const listed = await listTree(home, root, ".", context);
        if (!listed.ok) throw new Error(treeFailureText(listed.error));
        const listing = new Map(
            listed.value.files.map((f) => [f.path, f.sha256]),
        );
        const paths = merge.brought
            .filter(
                (f) =>
                    listing.get(f.path) === f.sha256 &&
                    (clean || dirname(f.path) !== `merged/${entry.agent.name}`),
            )
            .map((f) => f.path);
        if (paths.length) {
            const treeOutcome3 = await removeTree(home, root, paths, context);
            if (!treeOutcome3.ok)
                throw new Error(treeFailureText(treeOutcome3.error));
            const trimmed = await env.exec(
                [
                    process.execPath,
                    "-e",
                    `const fs=require("node:fs"),p=require("node:path"); const [root,...files]=process.argv.slice(1); for(const file of files) { let dir=p.dirname(p.join(root,file)); while(dir!==root && dir.startsWith(root+p.sep)) { try { fs.rmdirSync(dir); } catch { break; } dir=p.dirname(dir); } }`,
                    root,
                    ...paths,
                ],
                undefined,
                context,
            );
            if (!trimmed.ok || trimmed.value.exitCode !== 0)
                throw new Error(
                    "The split's empty folders could not be removed safely.",
                );
        }
        await home.harness.commit(async (tx) => {
            const a = (await tx.doc(RecordDoc, hamlet, hamlet)).archive.find(
                (a) => a.id === entry.id,
            );
            if (a?.merge) a.merge.applied = true;
        }, context);
    } finally {
        await env.cleanup(context);
    }
    return { ok: true };
}

type PendingFiles = {
    kind: "pending-files" | "changed-notes";
    operation: "merge" | "split";
    hamlet: string;
    archive: string;
    kept: string;
    absorbed: string;
    retrySafe: true;
};

type FileResult =
    | { ok: true }
    | { ok: false; error: PendingFiles | NotesFailure };

function pendingText(error: PendingFiles | NotesFailure): string {
    switch (error.kind) {
        case "notes-unreadable":
            return notesFailureText(error);
        case "changed-notes":
            return `The merge of \`${error.archive}\` is pending because the kept agent's notes changed. Both versions are kept. Save its newer NOTES.md aside and put its merged/${error.absorbed}/NOTES.before.md back as NOTES.md, then use /merge ${error.archive} into ${error.kept} to retry safely. Do not erase its house.`;
        case "pending-files":
            return `The ${error.operation} of \`${error.archive}\` in \`${error.hamlet}\` is recorded but its files are still pending. Inspect both houses and keep conflicting paths aside, then use ${error.operation === "merge" ? `/merge ${error.archive} into ${error.kept}` : `/restore ${error.archive}`} to retry safely. Do not erase its house.`;
    }
    const unhandled: never = error;
    throw new Error(`Unclassified pending files: ${unhandled}`);
}

async function retryFiles(
    home: Home,
    hamlet: string,
    entry: Archived,
    context: Context,
): Promise<{ ok: true } | { ok: false; error: PendingFiles | NotesFailure }> {
    const merge = entry.merge;
    if (!merge) throw new Error("File work needs a merge archive.");
    let failure: PendingFiles | NotesFailure | undefined;
    try {
        failure = await stoppedHouses(
            home,
            hamlet,
            [entry.agent, merge.kept],
            context,
            async () => {
                if (entry.back === null) {
                    const outcome = await apply(home, hamlet, entry, context);
                    return outcome.ok ? undefined : outcome.error;
                }
                const outcome = await part(home, hamlet, entry, context);
                return outcome.ok ? undefined : outcome.error;
            },
        );
    } catch {
        failure = {
            kind: "pending-files",
            operation: entry.back === null ? "merge" : "split",
            hamlet,
            archive: entry.id,
            kept: merge.kept.name,
            absorbed: entry.agent.name,
            retrySafe: true,
        };
    }
    if (failure) {
        const text = safeWords(home, pendingText(failure));
        console.error(text);
        await home.harness.commit(
            (tx) => notifyChiefs(home, tx, hamlet, text),
            context,
        );
        return { ok: false, error: failure };
    }
    return { ok: true };
}

function notesFailureText(error: NotesFailure): string {
    switch (error.kind) {
        case "notes-unreadable":
            return `The ${error.operation} cannot use the kept agent's NOTES.md (${agentKey(error.agent)}): ${error.reason === "not-utf8" ? "it is not UTF-8 text" : "it could not be read safely"}. NOTES.md was not replaced. Keep its original bytes aside and correct the file, then read /record before retrying /merge or /restore <archive> for pending work.`;
    }
    const unhandled: never = error.kind;
    throw new Error(`Unclassified notes failure: ${unhandled}`);
}
