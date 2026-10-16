import { isUtf8 } from "node:buffer";
import { createHash } from "node:crypto";
import type { AgentRecord, Archived, RecordRefusal } from "../record.ts";
import { recordRefusal } from "../record.ts";
import { ok, type Result } from "../result.ts";

/** The notes and no-clobber files to keep from a merge. */
export type Planned = {
    files: Map<string, Buffer>;
    clashes: string[];
    before: string;
    after: string;
    notes: "joined" | "rewritten" | "plain";
};

function substance(text: string): string {
    return text
        .split("\n")
        .filter((line) => line.trim() !== "# Notes")
        .join("\n")
        .trim();
}
/** A file identity kept in the merge journal. */
export function digest(bytes: Buffer | string): string {
    return createHash("sha256").update(bytes).digest("hex");
}

/** Plans a merge from confined source bytes and the kept house's file identities. */
export function planMerge(given: {
    hamlet: string;
    gone: AgentRecord;
    before: Buffer;
    source: ReadonlyMap<string, Buffer>;
    target: ReadonlyMap<string, string>;
}): Result<Planned, RecordRefusal> {
    const present = given.target;
    const { hamlet, gone } = given;
    const from = [...given.source].map(([path, bytes]) => ({
        path,
        bytes,
        sha256: digest(bytes),
    }));
    const targetFiles = [...present].map(([path, sha256]) => ({
        path,
        sha256,
    }));
    const files = new Map<string, Buffer>();
    const clashes: string[] = [];
    const put = (rel: string, bytes: Buffer): Result<void, RecordRefusal> => {
        if (
            rel.startsWith("/") ||
            rel.includes("\\") ||
            rel.includes("\0") ||
            rel.split("/").some((p) => !p || p === "." || p === "..")
        )
            return recordRefusal("merge", hamlet, { kind: "merge-path" });
        if (files.has(rel) || present.has(rel))
            return recordRefusal("merge", hamlet, {
                kind: "merge-path-taken",
                path: rel,
            });
        files.set(rel, bytes);
        return ok(undefined);
    };
    const before = given.before.toString("utf8");
    const theirBytes = given.source.get("NOTES.md") ?? Buffer.from("# Notes\n");
    const theirs = theirBytes.toString("utf8");
    const both = !!substance(before) && !!substance(theirs);
    const body = substance(theirs)
        ? `${before.trimEnd()}\n\n## From ${gone.name}\n${substance(theirs)}`
        : before.trimEnd();
    const after = `${body.trimEnd()}\n\n${mergedParagraph(gone)}`;
    const beforeSaved = put(
        `merged/${gone.name}/NOTES.before.md`,
        given.before,
    );
    if (!beforeSaved.ok) return beforeSaved;
    const theirsSaved = put(`merged/${gone.name}/NOTES.md`, theirBytes);
    if (!theirsSaved.ok) return theirsSaved;
    const renamed = new Map<string, string>();
    for (const f of from) {
        const skill = /^skills\/([^/]+)\//.exec(f.path)?.[1];
        if (!skill || renamed.has(skill)) continue;
        const prefix = `skills/${skill}/`;
        const theirs = from.filter((f) => f.path.startsWith(prefix));
        const mine = targetFiles.filter((f) => f.path.startsWith(prefix));
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
    for (const f of from) {
        if (f.path === "NOTES.md") continue;
        let rel = f.path;
        let bytes = f.bytes;
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
            clashes.push(`file \`${f.path}\` (the absorbed one is \`${rel}\`)`);
        }
        const placed = put(rel, bytes);
        if (!placed.ok) return placed;
    }
    return ok({
        files,
        clashes,
        before,
        after,
        notes: both ? "joined" : "plain",
    });
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

/** Reconstructs a journaled file from the absorbed house, checking its kept identity. */
export function broughtBytes(
    source: ReadonlyMap<string, Buffer>,
    entry: Archived,
    f: { path: string; sha256: string },
): Buffer | undefined {
    const aside = `merged/${entry.agent.name}/`;
    if (f.path === `${aside}NOTES.before.md`)
        return Buffer.from(entry.merge?.notesBefore ?? "");
    const rel = f.path.startsWith(aside) ? f.path.slice(aside.length) : f.path;
    const candidates = [f.path, rel];
    for (const path of candidates) {
        const bytes = source.get(path);
        if (bytes && digest(bytes) === f.sha256) return bytes;
    }
    const name = /^skills\/([^/]+)\//.exec(rel)?.[1];
    const suffix = `-from-${entry.agent.name}`;
    if (name?.endsWith(suffix)) {
        const old = name.slice(0, -suffix.length);
        const path = rel.replace(`skills/${name}/`, `skills/${old}/`);
        const bytes = source.get(path);
        if (bytes) {
            const changed = renamedSkill(bytes, old, name);
            if (digest(changed) === f.sha256) return changed;
        }
    }
    if (rel === "NOTES.md" && digest("# Notes\n") === f.sha256)
        return Buffer.from("# Notes\n");
    return undefined;
}

/** The kept house's pointer to both original notes and differing files. */
export function mergedParagraph(gone: AgentRecord): string {
    return `## Merged\n- \`${gone.name}\` was merged into me. Its purpose was: ${gone.purpose} Its notes as they were are in merged/${gone.name}/NOTES.md and mine in merged/${gone.name}/NOTES.before.md. Files of its that differed from mine of the same name are under merged/${gone.name}/.\n`;
}

/** Accepts a nonempty notes rewrite within the hamlet's character cap. */
export function accept(reply: string, limit: number): string | undefined {
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
