import { realpathSync } from "node:fs";
import { homedir } from "node:os";
import { isAbsolute, join } from "node:path";
import type { Context } from "@earendil-works/chord";

import type { Home } from "../home.ts";
import { RecordDoc } from "../record.ts";
import { err, ok, type Result } from "../result.ts";
import { canon, isDir, within } from "../sandbox/policy.ts";

/** A work folder rejected before the hamlet changes its reach. */
export type WorkFailure = {
    operation: "set-folder";
    hamlet: string;
    retrySafe: true;
} & (
    | { kind: "folder-not-absolute" }
    | { kind: "folder-not-directory"; path: string }
    | { kind: "folder-overlaps-home"; root: string }
);

/** The chief's unchanged folder refusal text. */
export function workFailureText(error: WorkFailure): string {
    switch (error.kind) {
        case "folder-not-absolute":
            return "give the folder's full path, such as /Users/you/code/project";
        case "folder-not-directory":
            return `\`${error.path}\` is not a folder`;
        case "folder-overlaps-home":
            return `a hamlet cannot work on Hamlet's own folder (${error.root}), or on one that holds it`;
    }
    const unhandled: never = error;
    throw new Error(`Unclassified work failure: ${unhandled}`);
}

/** The hamlet's mode in a sentence for the chief (C21). */
export function modeWords(mode: "manual" | "auto"): string {
    return mode === "manual"
        ? "Agents ask you before each edit and command."
        : "Agents edit and run commands in the folder without asking.";
}

/** The folder decree, with full paths checked before the hamlet's work changes (C20). */
export async function folder(
    home: Home,
    hamlet: string,
    typed: string,
    context: Context,
): Promise<Result<string, WorkFailure>> {
    if (typed === "") {
        const work =
            (await home.harness.snapshot(WorkDoc, hamlet, context)) ??
            WorkDoc.definition.initial(hamlet);
        return ok(
            work.folder === null
                ? "This hamlet works on no folder. `/folder <folder>` points it at one."
                : `This hamlet works on \`${work.folder}\`. ${modeWords(work.mode)} \`/folder none\` stops that.`,
        );
    }
    let next: string | null = null;
    if (typed !== "none") {
        const expanded =
            typed === "~"
                ? homedir()
                : /^~[/\\]/.test(typed)
                  ? join(homedir(), typed.slice(2))
                  : typed;
        if (!isAbsolute(expanded))
            return err({
                kind: "folder-not-absolute",
                operation: "set-folder",
                hamlet,
                retrySafe: true,
            });
        if (!isDir(expanded))
            return err({
                kind: "folder-not-directory",
                path: expanded,
                operation: "set-folder",
                hamlet,
                retrySafe: true,
            });
        next = realpathSync.native(expanded);
        const root = canon(home.dir);
        if (within(next, root) || within(root, next))
            return err({
                kind: "folder-overlaps-home",
                root,
                operation: "set-folder",
                hamlet,
                retrySafe: true,
            });
    }
    const mode = await home.harness.commit(async (tx) => {
        const work = await tx.doc(WorkDoc, hamlet, hamlet);
        work.before = work.before.filter((old) => old !== next);
        if (work.folder !== null && work.folder !== next)
            work.before.unshift(work.folder);
        work.folder = next;
        (await tx.doc(RecordDoc, hamlet, hamlet)).decisions.push({
            kind: "folder_set",
            at: home.now(),
            by: home.chief,
            folder: next,
        });
        return work.mode;
    }, context);
    return ok(
        next === null
            ? "This hamlet works on no folder now. Its agents are back in their own workspaces."
            : `This hamlet works on \`${next}\` now. ${modeWords(mode)}`,
    );
}

/** The mode decree, kept whichever folder the hamlet works on (C21). */
export async function setMode(
    home: Home,
    hamlet: string,
    mode: "manual" | "auto",
    context: Context,
): Promise<string> {
    const working = await home.harness.commit(async (tx) => {
        const work = await tx.doc(WorkDoc, hamlet, hamlet);
        work.mode = mode;
        (await tx.doc(RecordDoc, hamlet, hamlet)).decisions.push({
            kind: "mode_set",
            at: home.now(),
            by: home.chief,
            mode,
        });
        return work.folder !== null;
    }, context);
    return (
        modeWords(mode) +
        (working ? "" : " It counts once this hamlet works on a folder.")
    );
}

import { defineDocFamily } from "@earendil-works/pi-durable";

/** The folder a hamlet works on, those it worked on before, and whether its agents ask before changing it (C20, C21). */
export type Work = {
    folder: string | null;
    mode: "manual" | "auto";
    before: string[];
};

/** Each hamlet's work, by hamlet id. */
export const WorkDoc = defineDocFamily<Work, string>({
    kind: "hamlet.work",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ folder: null, mode: "manual", before: [] }),
});
