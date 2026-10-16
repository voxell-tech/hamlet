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
import { tellChiefs } from "../chats/conversations.ts";
import { stopAgent } from "../chats.ts";
import { treeWorker } from "../environment.ts";
import { houseOf } from "../home/layout.ts";
import type { Home } from "../home.ts";
import {
    list as listTree,
    MERGE_CAPS,
    remove as removeTree,
    stage as stageTree,
    type TreeFailure,
    treeFailureText,
} from "../house.ts";
import {
    type AgentRecord,
    type AgentRef,
    type Archived,
    agentKey,
    RecordDoc,
    type RecordRefusal,
    recordRefusal,
    toolText,
} from "../record.ts";
import { ok, type Result } from "../result.ts";
import type { SandboxEnv } from "../sandbox.ts";
import { broughtBytes, type Planned, planMerge } from "./merge-plan.ts";

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
    | NotesFailure
    | RecordRefusal
    | TreeFailure;
type Prepared = Result<Planned, PrepareFailure>;

/** The safe refusal before any merge is recorded. */
export function prepareText(error: PrepareFailure): string {
    switch (error.kind) {
        case "notes-unreadable":
            return notesFailureText(error);
        case "destination-blocked":
            return `The merge cannot safely deliver \`${error.path}\`; keep its destination and any linked parent aside before merging.`;
    }
    switch (error.kind) {
        case "entries-cap":
        case "bytes-cap":
        case "unsafe-entry":
        case "stage-not-empty":
        case "stage-overlaps-source":
        case "path-invalid":
        case "clobber":
        case "shelf-linked":
        case "refused":
            return treeFailureText(error);
        default:
            return toolText(error);
    }
}

async function notes(
    home: Home,
    ref: AgentRef,
    context: Context,
    operation: "merge" | "split" = "merge",
): Promise<Result<{ text: string; bytes: Buffer }, NotesFailure>> {
    const read = await home.houses.of(ref).notes(context);
    if (read.ok) return read;
    const facts = {
        kind: "notes-unreadable",
        operation,
        agent: ref,
        retrySafe: true,
    } as const;
    switch (read.error.kind) {
        case "notes-missing":
            return {
                ok: true,
                value: { text: "# Notes\n", bytes: Buffer.from("# Notes\n") },
            };
        case "notes-not-utf8":
            return { ok: false, error: { ...facts, reason: "not-utf8" } };
        case "notes-unreadable":
            return { ok: false, error: { ...facts, reason: "unavailable" } };
    }
    const unhandled: never = read.error;
    throw new Error(`Unclassified notes read: ${unhandled}`);
}

/** Reads confined house inputs and checks every planned destination before recording a merge. */
export async function prepare(
    home: Home,
    hamlet: string,
    { kept, gone }: { kept: AgentRecord; gone: AgentRecord },
    context: Context,
): Promise<Prepared> {
    const root = houseOf(home.layout, { hamlet, number: gone.number });
    const target = houseOf(home.layout, { hamlet, number: kept.number });
    const stage = await mkdtemp(join(tmpdir(), "hamlet-merge-"));
    try {
        const staged = await stageTree(
            home,
            { root, rel: "." },
            stage,
            MERGE_CAPS,
            context,
        );
        if (!staged.ok) {
            if (
                staged.error.kind === "entries-cap" ||
                staged.error.kind === "bytes-cap"
            )
                return recordRefusal("merge", hamlet, {
                    kind: "merge-too-large",
                });
            return staged;
        }
        const from = await listTree(home, stage, ".", context);
        if (!from.ok) return from;
        const to = await listTree(home, target, ".", context);
        if (!to.ok) return to;
        const keptNotes = await notes(
            home,
            { hamlet, number: kept.number },
            context,
        );
        if (!keptNotes.ok) return keptNotes;
        const source = new Map<string, Buffer>();
        for (const file of from.value.files)
            source.set(file.path, await readFile(join(stage, file.path)));
        const planned = planMerge({
            hamlet,
            gone,
            before: keptNotes.value.bytes,
            source,
            target: new Map(to.value.files.map((f) => [f.path, f.sha256])),
        });
        if (!planned.ok) return planned;
        const { files } = planned.value;
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
        return planned;
    } finally {
        await rm(stage, { recursive: true, force: true });
    }
}

function mergeWriter(home: Home, ref: AgentRef): SandboxEnv {
    const root = houseOf(home.layout, ref);
    return treeWorker(home, { root, stage: root, caps: MERGE_CAPS });
}

async function apply(
    home: Home,
    hamlet: string,
    entry: Archived,
    context: Context,
): Promise<FileResult> {
    const merge = entry.merge;
    if (!merge || merge.applied) return ok(undefined);
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
        if (!treeOutcome2.ok) return pendingFiles(hamlet, entry);
        const sourceTree = await listTree(home, source, ".", context);
        if (!sourceTree.ok) return pendingFiles(hamlet, entry);
        const sourceBytes = new Map<string, Buffer>();
        for (const file of sourceTree.value.files)
            sourceBytes.set(file.path, await readFile(join(source, file.path)));
        const listed = await listTree(home, target, ".", context);
        if (!listed.ok) return pendingFiles(hamlet, entry);
        const present = new Map(
            listed.value.files.map((f) => [f.path, f.sha256]),
        );
        const pending: string[] = [];
        for (const f of merge.brought) {
            if (present.get(f.path) === f.sha256) continue;
            if (present.has(f.path)) return pendingFiles(hamlet, entry);
            const bytes = broughtBytes(sourceBytes, entry, f);
            if (!bytes) return pendingFiles(hamlet, entry);
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
        const env = mergeWriter(home, { hamlet, number: merge.kept.number });
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
            if (!result.ok) return pendingFiles(hamlet, entry);
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
    return ok(undefined);
}

async function part(
    home: Home,
    hamlet: string,
    entry: Archived,
    context: Context,
): Promise<FileResult> {
    const merge = entry.merge;
    if (!merge || merge.applied) return ok(undefined);
    const ref = { hamlet, number: merge.kept.number };
    await stopAgent(home, ref, context);
    const root = houseOf(home.layout, ref);
    const env = mergeWriter(home, ref);
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
        if (!written.ok) return pendingFiles(hamlet, entry);
        const listed = await listTree(home, root, ".", context);
        if (!listed.ok) return pendingFiles(hamlet, entry);
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
            if (!treeOutcome3.ok) return pendingFiles(hamlet, entry);
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
                return pendingFiles(hamlet, entry);
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
    return ok(undefined);
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

type FileResult = Result<void, PendingFiles | NotesFailure>;

function pendingFiles(
    hamlet: string,
    entry: Archived,
): { ok: false; error: PendingFiles } {
    const merge = entry.merge;
    if (!merge) throw new Error("File work requires a checked merge manifest.");
    return {
        ok: false,
        error: {
            kind: "pending-files",
            operation: entry.back === null ? "merge" : "split",
            hamlet,
            archive: entry.id,
            kept: merge.kept.name,
            absorbed: entry.agent.name,
            retrySafe: true,
        },
    };
}

/** Recovery advice for a journal whose file phase is still pending. */
export function pendingText(error: PendingFiles | NotesFailure): string {
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

/** Retries the journal's file phase under the caller's house hold. */
export async function retryFiles(
    home: Home,
    hamlet: string,
    entry: Archived,
    context: Context,
    hold: <T>(
        home: Home,
        hamlet: string,
        agents: AgentRecord[],
        context: Context,
        action: () => Promise<T>,
    ) => Promise<T>,
): Promise<FileResult> {
    const merge = entry.merge;
    if (!merge) throw new Error("File work needs a merge archive.");
    let failure: PendingFiles | NotesFailure | undefined;
    try {
        failure = await hold(
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
        failure = pendingFiles(hamlet, entry).error;
    }
    if (failure) {
        const text = home.secrets.words(pendingText(failure));
        console.error(text);
        await home.harness.commit(
            (tx) => tellChiefs(home, tx, hamlet, text),
            context,
        );
        return { ok: false, error: failure };
    }
    return ok(undefined);
}

function notesFailureText(error: NotesFailure): string {
    switch (error.kind) {
        case "notes-unreadable":
            return `The ${error.operation} cannot use the kept agent's NOTES.md (${agentKey(error.agent)}): ${error.reason === "not-utf8" ? "it is not UTF-8 text" : "it could not be read safely"}. NOTES.md was not replaced. Keep its original bytes aside and correct the file, then read /record before retrying /merge or /restore <archive> for pending work.`;
    }
    const unhandled: never = error.kind;
    throw new Error(`Unclassified notes failure: ${unhandled}`);
}
