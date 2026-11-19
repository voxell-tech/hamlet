/**
 * A letter's or a question's parcel, saved in the agent's `inbox/` as
 * `crates/hamlet_server/src/inbox.rs` saves files sent to an agent.
 */
import type { Context } from "@earendil-works/chord/context";
import type { ParcelFile } from "./messages/agent.ts";
import type { Peers } from "./turn.ts";

const INBOX = "inbox";

/**
 * As `AttachedFile::CAP_BYTES`: a file is held whole before its write, so
 * a larger one is not fetched.
 */
const MOST_HELD = 32 * 1024 * 1024;

/** A file or folder saved, as the agent finds it. */
interface Saved {
    /** Its path from the agent's filesystem. */
    readonly path: string;
    files: number;
    bytes: number;
    folder: boolean;
}

/**
 * Saves each file of `files` under `inbox/`, a top name the folder holds
 * already numbered as today's, and gives the captions that say where each
 * landed. A file that is not saved is reported and has no caption.
 */
export async function received(
    peers: Peers,
    files: readonly ParcelFile[],
    context: Context,
): Promise<string> {
    const saved = new Map<string, Saved>();
    const taken = new Map<string, Set<string>>();
    const savedAt = async (folder: string, top: string): Promise<Saved> => {
        const key = `${folder}/${top}`;
        const known = saved.get(key);
        if (known !== undefined) return known;
        let names = taken.get(folder);
        if (names === undefined) {
            const listed = await peers.files.list(folder, context);
            names = new Set(
                listed.ok ? listed.value.map((entry) => entry.name) : [],
            );
            taken.set(folder, names);
        }
        let name = top;
        for (let n = 2; names.has(name); n += 1) name = numbered(top, n);
        names.add(name);
        const made = {
            path: `${folder}/${name}`,
            files: 0,
            bytes: 0,
            folder: false,
        };
        saved.set(key, made);
        return made;
    };
    /** Saves `sent`, or gives why it was not. */
    const save = async (sent: ParcelFile): Promise<unknown> => {
        const [from = "", top = "", ...rest] = sent.path.split("/");
        const bytes = await fetched(peers, sent);
        if (typeof bytes === "string") return bytes;
        const at = await savedAt(`${INBOX}/${from}`, top);
        const wrote = await peers.files.write(
            [at.path, ...rest].join("/"),
            bytes,
            context,
        );
        if (!wrote.ok) return wrote.error;
        at.files += 1;
        at.bytes += bytes.length;
        at.folder ||= rest.length > 0;
        return undefined;
    };
    for (const sent of files) {
        const why = await save(sent);
        if (why !== undefined) {
            peers.report(`${sent.path} was not saved in the inbox`, why);
        }
    }
    return [...saved.values()]
        .filter((at) => at.files > 0)
        .map((at, index) => {
            const where = `${peers.files.root}/${at.path}`;
            const size = sizeWords(at.bytes);
            return at.folder
                ? `[Folder ${index + 1} of this message, ${at.files} files, ${size}, saved at ${where}]`
                : `[File ${index + 1} of this message, ${size}, saved at ${where}]`;
        })
        .join("\n");
}

/** The file's bytes, part by part, or why it was not fetched. */
async function fetched(
    peers: Peers,
    { parcel, file, size }: ParcelFile,
): Promise<Buffer | string> {
    if (size > MOST_HELD) return `it holds more than ${sizeWords(MOST_HELD)}`;
    const parts: Buffer[] = [];
    for (let part = 0; ; part += 1) {
        const answer = await peers.reach.fetch_part({ parcel, file, part });
        if (answer.type === "refused") return answer.words;
        parts.push(Buffer.from(answer.bytes, "base64"));
        if (answer.last) return Buffer.concat(parts);
    }
}

/** `name` made different by `n`: `report-2.pdf` for `report.pdf`. */
function numbered(name: string, n: number): string {
    const dot = name.lastIndexOf(".");
    return dot > 0
        ? `${name.slice(0, dot)}-${n}${name.slice(dot)}`
        : `${name}-${n}`;
}

/** As `AttachedFile::size_words`. */
function sizeWords(bytes: number): string {
    if (bytes < 1024) return `${bytes} bytes`;
    const units = ["KiB", "MiB", "GiB"];
    let size = bytes / 1024;
    let unit = 0;
    while (size >= 1024 && unit + 1 < units.length) {
        size /= 1024;
        unit += 1;
    }
    return `${size.toFixed(1)} ${units[unit]}`;
}
