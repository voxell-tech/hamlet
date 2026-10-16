import { lstat } from "node:fs/promises";
import { dirname, isAbsolute, join, win32 } from "node:path";
import { MAX_GIFT_BYTES } from "../protocol.ts";
import { err, ok, type Result } from "../result.ts";
import type { TreeFailure } from "./trees.ts";

/** A chief's gift was refused before admission or could not finish safely. */
export type GiftRefusal = { operation: "give"; retrySafe: false } & (
    | {
          kind:
              | "skill-name"
              | "stage-refused"
              | "uploads-full"
              | "bytes-full"
              | "part-mismatch"
              | "upload-unknown"
              | "connection-closed"
              | "recipient-changed";
      }
    | { kind: "gift-agent-missing"; agent: string }
    | { kind: "skill-exists"; agent: string; skill: string }
    | {
          kind: "skill-missing";
          agent: string;
          skill: string;
          starter: boolean;
          target: string;
      }
    | {
          kind: "gift-manifest";
          reason:
              | "empty"
              | "entries"
              | "path"
              | "bytes"
              | "duplicate"
              | "file-parent";
      }
    | { kind: "skill-tree" | "gift-tree"; failure: TreeFailure }
);

/** Safe wording for gift admission and delivery failures. */
export function giftRefusalText(error: GiftRefusal): string {
    switch (error.kind) {
        case "skill-name":
            return "a skill is named in letters, digits and hyphens";
        case "stage-refused":
            return "The gift could not be staged. Nothing was handed over.";
        case "uploads-full":
            return "This connection already has three file gifts waiting. Finish one before starting another.";
        case "bytes-full":
            return "This agent already has 32 MB of file gifts waiting. Finish or discard one before starting another.";
        case "part-mismatch":
            return "A gift part must match its path, next offset, size and base64 bytes.";
        case "upload-unknown":
            return "There is no file gift here with that upload id. It may have expired.";
        case "connection-closed":
            return "The gift's connection closed.";
        case "recipient-changed":
            return "The receiving agent is no longer present.";
        case "gift-agent-missing":
            return `no agent named \`${error.agent}\``;
        case "skill-exists":
            return `\`${error.agent}\` already has a skill named \`${error.skill}\``;
        case "skill-missing":
            return error.starter
                ? `no starter skill named \`${error.skill}\`. Say whose it is: /give ${error.target} ${error.skill} from <agent>`
                : `\`${error.agent}\` has no skill named \`${error.skill}\``;
        case "gift-tree":
            return "The gift could not be delivered inside the inbox or skills folder. Nothing was handed over.";
        case "skill-tree":
            switch (error.failure.kind) {
                case "entries-cap":
                    return "A skill gift takes at most 4096 files and folders at once.";
                case "bytes-cap":
                    return "The skill gift is larger than 32 MB.";
                case "unsafe-entry":
                    return "A skill gift cannot include links or special files.";
                case "stage-not-empty":
                case "stage-overlaps-source":
                case "path-invalid":
                case "clobber":
                case "shelf-linked":
                case "refused":
                    return "The gift could not be delivered inside the inbox or skills folder. Nothing was handed over.";
            }
            break;
        case "gift-manifest": {
            switch (error.reason) {
                case "empty":
                    return "Choose a file or folder to hand over.";
                case "entries":
                    return "A gift takes at most 4096 files at once.";
                case "path":
                    return "A gift's path must be relative to the inbox, with ordinary file and folder names.";
                case "bytes":
                    return "The gift is larger than the 32 MB an inbox takes at once.";
                case "duplicate":
                    return "The gift repeats a path, ignoring case and Unicode spelling.";
                case "file-parent":
                    return "The gift uses one path as both a file and a folder.";
            }
            const reason: never = error.reason;
            throw new Error(`Unclassified gift manifest: ${reason}`);
        }
    }
    const unhandled: never = error;
    throw new Error(`Unclassified gift failure: ${unhandled}`);
}

/** A safe gift reservation, with collisions folded across case and Unicode spelling. */
export function manifestSize(
    files: { path: string; size: number }[],
): Result<number, GiftRefusal> {
    const refuse = (
        reason: Extract<GiftRefusal, { kind: "gift-manifest" }>["reason"],
    ) =>
        err({
            kind: "gift-manifest",
            reason,
            operation: "give",
            retrySafe: false,
        } as const);
    if (!files.length) return refuse("empty");
    if (files.length > 4096) return refuse("entries");
    let size = 0;
    const paths = new Set<string>();
    for (const file of files) {
        if (
            isAbsolute(file.path) ||
            win32.isAbsolute(file.path) ||
            file.path.includes("\\") ||
            file.path.includes("\0") ||
            file.path
                .split("/")
                .some(
                    (part) =>
                        !part ||
                        part === "." ||
                        part === ".." ||
                        Buffer.byteLength(part) > 255,
                )
        )
            return refuse("path");
        size += file.size;
        if (size > MAX_GIFT_BYTES) return refuse("bytes");
        const folded = file.path
            .normalize("NFC")
            .toLowerCase()
            .toUpperCase()
            .toLowerCase()
            .normalize("NFC");
        if (paths.has(folded)) return refuse("duplicate");
        paths.add(folded);
    }
    for (const path of paths)
        for (
            let parent = dirname(path);
            parent !== ".";
            parent = dirname(parent)
        )
            if (paths.has(parent)) return refuse("file-parent");
    return ok(size);
}

/** Refuses linked receiving shelves before any bytes are staged. */
export async function checkShelf(
    house: string,
    shelf: string,
    roots: string[],
): Promise<Result<void, GiftRefusal>> {
    const failure: TreeFailure = {
        kind: "shelf-linked",
        operation: "deliver-tree",
        retrySafe: false,
    };
    const refuse = () =>
        err({
            kind: "gift-tree",
            failure,
            operation: "give",
            retrySafe: false,
        } as const);
    try {
        const dir = join(house, shelf);
        const info = await lstat(dir).catch(() => undefined);
        if (info !== undefined && !info.isDirectory()) return refuse();
        for (const root of roots)
            if (
                (
                    await lstat(join(dir, root)).catch(() => undefined)
                )?.isSymbolicLink()
            )
                return refuse();
        return ok(undefined);
    } catch {
        return refuse();
    }
}
