import { realpathSync } from "node:fs";
import { homedir } from "node:os";
import { isAbsolute, join } from "node:path";
import type { Context } from "@earendil-works/chord";

import type { Home } from "../home.ts";
import { RecordDoc } from "../record.ts";
import { canon, isDir, within } from "../sandbox.ts";

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
): Promise<string> {
    if (typed === "") {
        const work =
            (await home.harness.snapshot(WorkDoc, hamlet, context)) ??
            WorkDoc.definition.initial(hamlet);
        return work.folder === null
            ? "This hamlet works on no folder. `/folder <folder>` points it at one."
            : `This hamlet works on \`${work.folder}\`. ${modeWords(work.mode)} \`/folder none\` stops that.`;
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
            throw new Error(
                "give the folder's full path, such as /Users/you/code/project",
            );
        if (!isDir(expanded))
            throw new Error(`\`${expanded}\` is not a folder`);
        next = realpathSync.native(expanded);
        const root = canon(home.dir);
        if (within(next, root) || within(root, next))
            throw new Error(
                `a hamlet cannot work on Hamlet's own folder (${root}), or on one that holds it`,
            );
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
    return next === null
        ? "This hamlet works on no folder now. Its agents are back in their own workspaces."
        : `This hamlet works on \`${next}\` now. ${modeWords(mode)}`;
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
