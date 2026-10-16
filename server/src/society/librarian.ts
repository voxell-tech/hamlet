import { mkdtemp, readFile, rm } from "node:fs/promises";
import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { Type } from "@earendil-works/pi-ai";
import {
    type ConversationId,
    defineDoc,
    defineExtension,
    defineTool,
    type TaskId,
    type ToolExecutionApi,
} from "@earendil-works/pi-durable";
import { WithDoc } from "../chats.ts";
import { runErrand, startErrand } from "../errands.ts";
import {
    deliverTree,
    listTree,
    removeTree,
    stageSkill,
} from "../gift-files.ts";
import type { Home } from "../home.ts";
import { houseOf } from "../layout.ts";
import { booksOf } from "../ledger.ts";
import { libraryPart } from "../library.ts";
import { LimitsDoc, limitsOf } from "../limits.ts";
import type { Deed, Part } from "../parts.ts";
import type { State } from "../protocol.ts";
import { RecordDoc } from "../record.ts";
import { stopped } from "../report.ts";
import { parseSkill, refreshSkills, skillsIn } from "../skills.ts";
import { turnOf } from "../turn.ts";
import { LibraryDoc } from "./docs.ts";

const ConsultDoc = defineDoc<{
    errand: ConversationId | null;
    stage: string | null;
    skill: string | null;
    by: string;
    task: string;
    question: string;
    answer: string | null;
    delivered: boolean;
    result: string | null;
}>({
    kind: "hamlet.library-consult",
    version: 1,
    scope: "task",
    initial: () => ({
        errand: null,
        stage: null,
        skill: null,
        by: "",
        task: "",
        question: "",
        answer: null,
        delivered: false,
        result: null,
    }),
});

const ClaimsDoc = defineDoc<{ skills: Record<string, TaskId> }>({
    kind: "hamlet.library-claims",
    version: 1,
    scope: "session",
    initial: () => ({ skills: {} }),
});

/** The librarian's shelf and advice (H33). */
export const Librarian: Part = (home) => ({
    extension: defineExtension({
        name: "hamlet.librarian",
        tools: [
            defineTool({
                name: "submit_skill",
                description:
                    "Offer one of your own skills to the shared library. The librarian takes it or says why not. Once taken it moves out of your house; use the library's copy.",
                parameters: Type.Object({
                    skill: Type.String(),
                    reason: Type.String(),
                }),
                replay: "safe",
                execute: async (args, api, context) =>
                    answer(
                        await consult(
                            home,
                            api,
                            { skill: args.skill, text: args.reason },
                            context,
                        ),
                    ),
            }),
            defineTool({
                name: "ask_librarian",
                description:
                    "Ask which skill on the library's shelves does what you need, and the command that runs it. Ask before writing a skill of your own.",
                parameters: Type.Object({ question: Type.String() }),
                replay: "safe",
                execute: async (args, api, context) =>
                    answer(
                        await consult(
                            home,
                            api,
                            { text: args.question },
                            context,
                        ),
                    ),
            }),
        ],
    }),
});

function answer(text: string) {
    return { output: [{ type: "text" as const, text }] };
}

async function consult(
    home: Home,
    api: ToolExecutionApi,
    given: { skill?: string; text: string },
    context: Context,
): Promise<string> {
    await retireOffers(home, context);
    let job = await api.snapshot(ConsultDoc, api.taskId, context);
    if (job?.result != null) {
        if (job.stage) await rm(job.stage, { recursive: true, force: true });
        return job.result;
    }
    const place = await api.snapshot(WithDoc, api.conversationId, context);
    if (!place) return "Only an agent can ask the librarian.";
    const house = houseOf(home.layout, place);
    if (job?.errand == null) {
        if (given.skill !== undefined && !validName(given.skill))
            return "`skill` is its folder under skills/, in letters, digits, hyphens and underscores.";
        if (!given.text.trim())
            return given.skill === undefined
                ? "Say in `question` what you are looking for."
                : "Say in `reason` who else would use it.";
        const turn = await turnOf(home.harness, api, context);
        if (!turn?.task)
            return "The librarian needs a task to pay for its call.";
        const limits = limitsOf(
            await api.snapshot(LimitsDoc, place.hamlet, context),
        );
        const reason = stopped(
            limits,
            await booksOf(
                home,
                place.hamlet,
                limits.budget.period,
                turn.task,
                context,
            ),
        );
        if (reason) return reason;
        const record = await api.snapshot(RecordDoc, place.hamlet, context);
        const by = `${record?.agents.find((a) => a.number === place.number)?.name ?? "agent"}@${place.hamlet}`;
        let stage: string | null = null;
        try {
            const shelves = await skillsIn(home.layout.library);
            let question: string;
            if (given.skill !== undefined) {
                stage = await mkdtemp(join(home.dir, ".library-stage-"));
                await stageSkill(home, house, given.skill, stage, context);
                const paper = await readFile(
                    join(stage, given.skill, "SKILL.md"),
                    "utf8",
                ).catch(() => "");
                const skill = parseSkill(paper);
                if (!skill?.name.trim() || !skill.description.trim())
                    throw new Error(
                        `\`${given.skill}\` has no SKILL.md that opens with a name and a description`,
                    );
                if (shelves.some((s) => s.folder === given.skill))
                    throw new Error(
                        `the library already has a skill named \`${given.skill}\`. Use that one, or submit yours under another name if it does something else`,
                    );
                const files = await listTree(home, stage, given.skill, context);
                question = `You are the librarian. You keep the library: the skills every agent of every hamlet shares. \`${by}\` offers a skill for it, giving this reason: ${given.text.trim()}\n\nTake it only if it is all of these: useful to agents other than the one that wrote it; not something a skill on the shelves already does; clear enough that an agent who has never seen it could run it from its text alone; and free of anything private to one agent, such as names, keys or paths of its own.\n\nOn the shelves now:\n${shelves.map((s) => `- ${s.name}: ${s.description}`).join("\n")}\nThe skill offered, with its files (${files.files.map((f) => f.path).join(", ")}):\n\n${[...paper].slice(0, 6000).join("")}\n\nAnswer with ACCEPT or REFUSE as your first word, then one or two sentences of why. If you refuse, say what would change your answer.`;
            } else {
                question = `You are the librarian. You keep the library: the skills every agent of every hamlet shares. \`${by}\` asks you for a skill: ${given.text.trim()}\n\nThe shelves, each skill with the command that runs it:\n${(await libraryPart(home.layout.library)) ?? "(nothing yet)"}\nAnswer in a few sentences. Name the skill or skills that do it and give the exact command to run, as it is written above. If nothing on the shelves does it, say so plainly, and say that it may write a skill of its own and offer it with \`submit_skill\` afterwards. Do not invent a skill that is not on the shelves.`;
            }
            const notes = await readFile(
                join(home.layout.librarian, "NOTES.md"),
                "utf8",
            ).catch(() => "");
            await api.commit(async (tx) => {
                if (given.skill !== undefined) {
                    const claims = (await tx.doc(ClaimsDoc)).skills;
                    if (
                        claims[`skill:${given.skill}`] !== undefined &&
                        claims[`skill:${given.skill}`] !== api.taskId
                    )
                        throw new Error(
                            "Another offer of that skill is being judged.",
                        );
                    claims[`skill:${given.skill}`] = api.taskId;
                }
                const errand = await startErrand(home, tx, {
                    agent: place,
                    kind: "librarian",
                    own: false,
                    instructions: `${notes}\n\n${question}`,
                    tools: [],
                });
                Object.assign(await tx.doc(ConsultDoc, api.taskId), {
                    errand,
                    stage,
                    skill: given.skill ?? null,
                    by,
                    task: turn.task,
                    question,
                });
            }, context);
        } catch (error) {
            if (stage) await rm(stage, { recursive: true, force: true });
            return error instanceof Error
                ? error.message
                : "The skill could not be offered safely.";
        }
        job = await api.snapshot(ConsultDoc, api.taskId, context);
    }
    if (!job?.errand) throw new Error("The librarian's errand is missing.");
    if (job.answer === null) {
        const said = await runErrand(
            home,
            job.errand,
            {
                requestId: `library:${api.taskId}`,
                text: job.question,
                task: job.task,
            },
            context,
        );
        if (said.status !== "done") {
            const result = `The librarian could not answer: ${said.text}`;
            const skill = job.skill;
            await api.commit(async (tx) => {
                (await tx.doc(ConsultDoc, api.taskId)).result = result;
                if (skill !== null)
                    delete (await tx.doc(ClaimsDoc)).skills[`skill:${skill}`];
            }, context);
            if (job.stage)
                await rm(job.stage, { recursive: true, force: true });
            return result;
        }
        await api.commit(async (tx) => {
            (await tx.doc(ConsultDoc, api.taskId)).answer = said.text;
        }, context);
        job = { ...job, answer: said.text };
    }
    const said = job.answer ?? "";
    if (job.skill === null) {
        await api.commit(async (tx) => {
            (await tx.doc(ConsultDoc, api.taskId)).result = said;
        }, context);
        return said;
    }
    const first = /^[A-Za-z]+/.exec(said.trim())?.[0] ?? "";
    const taken = first.toLowerCase() === "accept";
    const why = said
        .trim()
        .slice(first.length)
        .replace(/^[^\p{L}\p{N}]+/u, "")
        .trim();
    if (taken && job.stage !== null) {
        if (!job.delivered) {
            try {
                await deliverTree(
                    home,
                    { root: home.layout.library, into: "." },
                    job.stage,
                    [job.skill],
                    context,
                );
            } catch (error) {
                // A replay may find the tree delivered before its marker was kept.
                const source = await listTree(
                    home,
                    job.stage,
                    job.skill,
                    context,
                );
                const target = await listTree(
                    home,
                    home.layout.library,
                    job.skill,
                    context,
                ).catch(() => undefined);
                if (
                    !target ||
                    target.skipped.length ||
                    JSON.stringify(source.files) !==
                        JSON.stringify(target.files)
                )
                    throw error;
            }
            await api.commit(async (tx) => {
                (await tx.doc(ConsultDoc, api.taskId)).delivered = true;
            }, context);
        }
        await removeTree(home, house, [`skills/${job.skill}`], context);
        refreshSkills(home.dir);
    }
    const result = taken
        ? `The librarian took \`${job.skill}\`: ${why} It is in the library now and no longer in your house. Use the library's, as every agent can.`
        : `The librarian did not take \`${job.skill}\`: ${why} It stays in your house.`;
    await api.commit(async (tx) => {
        const state = await tx.doc(ConsultDoc, api.taskId);
        if (state.result !== null) return;
        (await tx.doc(LibraryDoc)).events.push({
            kind: taken ? "shelved" : "refused",
            skill: job.skill ?? "",
            by: job.by,
            reason: why,
            at: home.now(),
        });
        state.result = result;
        delete (await tx.doc(ClaimsDoc)).skills[`skill:${job.skill ?? ""}`];
    }, context);
    if (job.stage) await rm(job.stage, { recursive: true, force: true });
    return result;
}

function validName(name: string): boolean {
    return /^[A-Za-z0-9_-]+$/.test(name);
}

/** A chief's removal of a shared skill. */
export const unshelve: Deed = async (home, given, context) => {
    const [name = "", ...words] = given.rest.trim().split(/\s+/);
    if (!validName(name)) throw new Error("usage: /unshelve <skill>");
    if (!(await skillsIn(home.layout.library)).some((s) => s.folder === name))
        throw new Error(`the library has no skill named \`${name}\``);
    await removeTree(home, home.layout.library, [name], context);
    await home.harness.commit(async (tx) => {
        (await tx.doc(LibraryDoc)).events.push({
            kind: "unshelved",
            skill: name,
            by: `chief:${given.chief}`,
            reason: words.join(" ") || "The chief took it out.",
            at: home.now(),
        });
    }, context);
    return `Took \`${name}\` out of the library.`;
};

/** The shared shelf for the window. */
export async function libraryView(
    home: Home,
    context: Context,
): Promise<NonNullable<State["library"]>> {
    const events =
        (await home.harness.snapshot(LibraryDoc, context))?.events ?? [];
    return (await skillsIn(home.layout.library)).map((s) => ({
        name: s.folder,
        description: s.description,
        by:
            events.findLast((e) => e.skill === s.folder && e.kind === "shelved")
                ?.by ?? "Hamlet",
    }));
}

async function retireOffers(home: Home, context: Context): Promise<void> {
    const claims =
        (await home.harness.snapshot(ClaimsDoc, context))?.skills ?? {};
    for (const [skill, owner] of Object.entries(claims)) {
        const task = await home.harness.getTask(owner, context);
        if (task && task.state.status !== "terminal") continue;
        const job = await home.harness.snapshot(ConsultDoc, owner, context);
        await home.harness.commit(async (tx) => {
            const claims = (await tx.doc(ClaimsDoc)).skills;
            if (claims[skill] === owner) delete claims[skill];
        }, context);
        if (job?.stage) await rm(job.stage, { recursive: true, force: true });
    }
}
