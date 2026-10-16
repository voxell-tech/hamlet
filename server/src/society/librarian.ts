import {
    chmod,
    lstat,
    mkdir,
    mkdtemp,
    readdir,
    readFile,
    rename,
    rm,
} from "node:fs/promises";
import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
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
    type TreeListing,
} from "../gift-files.ts";
import type { Deed, Home, Part } from "../home.ts";
import { houseOf } from "../home.ts";
import { booksOf } from "../ledger.ts";
import { libraryPart } from "../library.ts";
import { LimitsDoc, limitsOf } from "../limits.ts";
import type { State } from "../protocol.ts";
import { RecordDoc } from "../record.ts";
import { stopped } from "../report.ts";
import { SandboxEnv } from "../sandbox/env.ts";
import { closedPolicy } from "../sandbox/reach.ts";
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
    async start(context) {
        await retireOffers(home, context);
        await sweepStages(home, context);
    },
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
        if (job.stage) await discardStage(job.stage);
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
                const stages = join(home.dir, ".library-stages");
                await mkdir(stages, { recursive: true, mode: 0o700 });
                stage = await mkdtemp(join(stages, "offer-"));
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
            if (stage) await discardStage(stage);
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
            if (job.stage) await discardStage(job.stage);
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
    const trimmed = said.trim();
    const first = /[A-Za-z]+/.exec(trimmed)?.[0] ?? "";
    const taken = first.toLowerCase() === "accept";
    const why = (
        trimmed.startsWith(first) ? trimmed.slice(first.length) : trimmed
    )
        .replace(/^[^\p{L}\p{N}]+/u, "")
        .trim();
    let keptChanges = false;
    let cleanupWarning: string | null = null;
    if (taken && job.stage !== null) {
        const removable = await removableSkill(home, house, job.skill, context);
        if (!job.delivered) {
            const delivered = await publishSkill(
                home,
                api.taskId,
                job.stage,
                job.skill,
                context,
            );
            if (!delivered.ok) {
                switch (delivered.error.kind) {
                    case "shelf-conflict":
                        return `The skill \`${delivered.error.skill}\` could not be shelved: a different tree already occupies that name in the library. Its house copy was kept. Inspect both copies before removing one or offering the skill under another name.`;
                    case "shelf-delivery":
                        return `The skill \`${delivered.error.skill}\` could not be copied and placed on the shelf. Its house copy was kept. Inspect the library and the house copy before offering it again.`;
                }
                const unhandled: never = delivered.error.kind;
                throw new Error(`Unclassified shelf delivery: ${unhandled}`);
            }
        }
        await home.harness.commit(async (tx) => {
            const state = await tx.doc(ConsultDoc, api.taskId);
            if (state.result !== null) return;
            state.delivered = true;
            state.result = `The librarian took \`${job.skill}\`: ${why} It was shelved, but its house copy could not be removed yet. Inspect the house copy before changing or offering it again.`;
            (await tx.doc(LibraryDoc)).events.push({
                kind: "shelved",
                skill: job.skill ?? "",
                by: job.by,
                reason: why,
                at: home.now(),
            });
            delete (await tx.doc(ClaimsDoc)).skills[`skill:${job.skill}`];
        }, BACKGROUND_CONTEXT);
        const comparison = await compareHouseCopy(
            home,
            house,
            job.stage,
            job.skill,
            context,
        );
        if (!comparison.ok)
            cleanupWarning = cleanupFailureText(comparison.error);
        else {
            keptChanges = comparison.value === "changed";
            if (!keptChanges) {
                const removed = removable.ok
                    ? await removeHouseCopy(home, house, job.skill, context)
                    : removable;
                if (!removed.ok)
                    cleanupWarning = cleanupFailureText(removed.error);
            }
        }
        refreshSkills(home.dir);
    }
    const result = taken
        ? `The librarian took \`${job.skill}\`: ${why} ${cleanupWarning ?? (keptChanges ? "It is in the library now; your later changes stayed in your house." : "It is in the library now and no longer in your house.")} Use the library's, as every agent can.`
        : `The librarian did not take \`${job.skill}\`: ${why} It stays in your house.`;
    await api.commit(async (tx) => {
        const state = await tx.doc(ConsultDoc, api.taskId);
        if (!taken && state.result === null)
            (await tx.doc(LibraryDoc)).events.push({
                kind: "refused",
                skill: job.skill ?? "",
                by: job.by,
                reason: why,
                at: home.now(),
            });
        state.result = result;
        delete (await tx.doc(ClaimsDoc)).skills[`skill:${job.skill ?? ""}`];
    }, context);
    if (job.stage) await discardStage(job.stage);
    return result;
}

type ShelfDelivery =
    | { ok: true; value: null }
    | {
          ok: false;
          error: {
              kind: "shelf-conflict" | "shelf-delivery";
              operation: "shelve-skill";
              skill: string;
              request: TaskId;
              retrySafe: false;
          };
      };

async function publishSkill(
    home: Home,
    request: TaskId,
    stage: string,
    skill: string,
    context: Context,
): Promise<ShelfDelivery> {
    const incoming = join(home.layout.library, `.incoming-${request}`);
    try {
        const target = join(home.layout.library, skill);
        if (await lstat(target).catch(() => undefined)) {
            const source = await listTree(home, stage, skill, context);
            const shelved = await listTree(
                home,
                home.layout.library,
                skill,
                context,
            );
            if (
                shelved.skipped.length ||
                JSON.stringify(source.files) !== JSON.stringify(shelved.files)
            )
                return {
                    ok: false,
                    error: {
                        kind: "shelf-conflict",
                        operation: "shelve-skill",
                        skill,
                        request,
                        retrySafe: false,
                    },
                };
            return { ok: true, value: null };
        }
        await discardStage(incoming);
        await mkdir(incoming, { mode: 0o700 });
        await deliverTree(
            home,
            { root: incoming, into: "." },
            stage,
            [skill],
            context,
        );
        await rename(join(incoming, skill), target);
        return { ok: true, value: null };
    } catch {
        return {
            ok: false,
            error: {
                kind: "shelf-delivery",
                operation: "shelve-skill",
                skill,
                request,
                retrySafe: false,
            },
        };
    } finally {
        await discardStage(incoming);
    }
}

async function sweepStages(home: Home, context: Context): Promise<void> {
    const kept = new Set<string>();
    const claims =
        (await home.harness.snapshot(ClaimsDoc, context))?.skills ?? {};
    for (const owner of Object.values(claims)) {
        const job = await home.harness.snapshot(ConsultDoc, owner, context);
        if (job?.stage) kept.add(job.stage);
    }
    const stages = join(home.dir, ".library-stages");
    await mkdir(stages, { recursive: true, mode: 0o700 });
    for (const name of await readdir(stages)) {
        const stage = join(stages, name);
        if (!kept.has(stage)) await discardStage(stage);
    }
    for (const name of await readdir(home.dir)) {
        const stage = join(home.dir, name);
        if (name.startsWith(".library-stage-") && !kept.has(stage))
            await discardStage(stage);
    }
    for (const name of await readdir(home.layout.library))
        if (name.startsWith(".incoming-"))
            await discardStage(join(home.layout.library, name));
}

type CleanupFailure = {
    kind:
        | "house-copy-inspection"
        | "house-copy-readonly"
        | "house-copy-removal";
    operation: "shelve-skill";
    skill: string;
    retrySafe: false;
};
type CleanupResult =
    | { ok: true; value: null }
    | { ok: false; error: CleanupFailure };

function cleanupFailureText(error: CleanupFailure): string {
    switch (error.kind) {
        case "house-copy-inspection":
            return "It was shelved, but the staged and house copies could not be compared. Inspect the house copy before removing or offering it again.";
        case "house-copy-readonly":
            return "It was shelved, but its house copy could not be removed because a folder is read-only. Inspect the copy and its folder permissions before removing it.";
        case "house-copy-removal":
            return "It was shelved, but its house copy could not be removed fully. Inspect the remaining house files before removing or offering it again.";
    }
    const unhandled: never = error.kind;
    throw new Error(`Unclassified house cleanup: ${unhandled}`);
}

async function compareHouseCopy(
    home: Home,
    house: string,
    stage: string,
    skill: string,
    context: Context,
): Promise<
    | { ok: true; value: "same" | "changed" }
    | { ok: false; error: CleanupFailure }
> {
    let source: TreeListing, current: TreeListing;
    try {
        source = await listTree(home, stage, skill, context);
        current = await listTree(home, house, `skills/${skill}`, context);
    } catch {
        return {
            ok: false,
            error: {
                kind: "house-copy-inspection",
                operation: "shelve-skill",
                skill,
                retrySafe: false,
            },
        };
    }
    const changed =
        current.skipped.length > 0 ||
        JSON.stringify(source.files) !==
            JSON.stringify(
                current.files.map((f) => ({
                    ...f,
                    path: f.path.slice("skills/".length),
                })),
            );
    return { ok: true, value: changed ? "changed" : "same" };
}

async function removableSkill(
    home: Home,
    house: string,
    skill: string,
    context: Context,
): Promise<CleanupResult> {
    const policy = closedPolicy(house, [house, process.execPath]);
    policy.rw = [];
    const env = new SandboxEnv({ cwd: house, policy, sandbox: home.sandbox });
    try {
        const result = await env.exec(
            [
                process.execPath,
                "-e",
                `
const fs = require("node:fs"), path = require("node:path");
function check(p) {
    const info = fs.lstatSync(p);
    if (!info.isDirectory()) return;
    if ((info.mode & 0o300) !== 0o300) process.exit(20);
    for (const name of fs.readdirSync(p)) check(path.join(p, name));
}
const parent = fs.lstatSync(path.dirname(process.argv[1]));
if (!parent.isDirectory() || (parent.mode & 0o300) !== 0o300) process.exit(20);
check(process.argv[1]);
`,
                join(house, "skills", skill),
            ],
            undefined,
            context,
        );
        return result.ok && result.value.exitCode === 0
            ? { ok: true, value: null }
            : {
                  ok: false,
                  error: {
                      kind:
                          result.ok && result.value.exitCode === 20
                              ? "house-copy-readonly"
                              : "house-copy-inspection",
                      operation: "shelve-skill",
                      skill,
                      retrySafe: false,
                  },
              };
    } finally {
        await env.cleanup(context);
    }
}

async function removeHouseCopy(
    home: Home,
    house: string,
    skill: string,
    context: Context,
): Promise<CleanupResult> {
    try {
        await removeTree(home, house, [`skills/${skill}`], context);
        return { ok: true, value: null };
    } catch {
        return {
            ok: false,
            error: {
                kind: "house-copy-removal",
                operation: "shelve-skill",
                skill,
                retrySafe: false,
            },
        };
    }
}

async function discardStage(stage: string): Promise<void> {
    const info = await lstat(stage).catch(() => undefined);
    if (!info) return;
    if (info.isDirectory()) {
        await chmod(stage, info.mode | 0o700);
        for (const name of await readdir(stage))
            await discardStage(join(stage, name));
    }
    await rm(stage, { recursive: true, force: true });
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
        if (job?.stage) await discardStage(job.stage);
    }
}
