/**
 * A letter's or a question's parcel, and a chief's line's images and files,
 * saved in the agent's `inbox/`.
 */
import type { Context } from "@earendil-works/chord/context";
import type { ImageContent } from "@earendil-works/pi-ai";
import type { Input, ParcelFile } from "./messages/agent.ts";
import { u32 } from "./messages/shared.ts";
import type { ImagesTaken } from "./models.ts";
import type { Peers } from "./turn.ts";

const INBOX = "inbox";

/**
 * As `AttachedFile::MOST_FILE`, the most a workspace sends of one file. A
 * file is held whole before its write.
 */
const MOST_HELD = 256 * 1024 * 1024;

/** Words of an input's content: at a byte offset of its text, or after it. */
export interface Caption {
    readonly at?: number;
    readonly words: string;
}

/** The content an input was given besides its words. */
export interface Received {
    readonly captions: readonly Caption[];
    /** As the model is given them. */
    readonly images: readonly ImageContent[];
}

export const NOTHING_RECEIVED: Received = { captions: [], images: [] };

/** A file or folder saved, as the agent finds it. */
interface Saved {
    /** Its path from the agent's filesystem. */
    readonly path: string;
    readonly at: number | undefined;
    files: number;
    bytes: number;
    folder: boolean;
}

interface Placed {
    readonly file: ParcelFile;
    readonly folder: string;
    readonly path: string;
    readonly at?: number;
}

/** Saves each file of a letter's parcel under `inbox/<its sender>/`. */
export async function received(
    peers: Peers,
    files: readonly ParcelFile[],
    context: Context,
): Promise<Received> {
    const placed = files.map((file) => {
        const [from = "", ...path] = file.path.split("/");
        return { file, folder: `${INBOX}/${from}`, path: path.join("/") };
    });
    return { captions: await savedIn(peers, placed, context), images: [] };
}

/**
 * Saves each image of `line` at `inbox/images/<name>` and each file under
 * `inbox/`, in the order of their places. The images are given to the model
 * when it takes them, at most as many as it takes in a line, fitted; one that
 * cannot be is left out and reported.
 */
export async function attached(
    peers: Peers,
    line: Extract<Input, { type: "line" }>,
    taken: ImagesTaken | undefined,
    context: Context,
): Promise<Received> {
    const captions: Caption[] = [];
    const images: ImageContent[] = [];
    const sorted = [...(line.images ?? [])].sort(
        (a, b) => placeOf(a.image.at) - placeOf(b.image.at),
    );
    for (const [index, { image, file }] of sorted.entries()) {
        const path = `${INBOX}/${file.path}`;
        const bytes = await savedWhole(peers, file, path, context);
        if (bytes === undefined) continue;
        captions.push({
            ...(image.at === undefined ? {} : { at: image.at }),
            words: `[Image ${index + 1} of this message, saved at ${peers.files.root}/${path}]`,
        });
        if (taken === undefined || index >= taken.perLine) continue;
        const fitted = await peers
            .fitImage({
                image: bytes.toString("base64"),
                limits: {
                    width: u32(taken.width),
                    height: u32(taken.height),
                    bytes: u32(taken.bytes),
                },
            })
            .catch((error: unknown) => ({ type: "failed" as const, error }));
        if (fitted.type !== "image") {
            peers.report(
                "an image was left out",
                fitted.type === "unusable" ? fitted.words : fitted.error,
            );
            continue;
        }
        images.push({
            type: "image",
            data: fitted.data,
            mimeType: fitted.mime,
        });
    }
    const files = [...(line.files ?? [])]
        .sort((a, b) => placeOf(a.attached.at) - placeOf(b.attached.at))
        .map(({ attached, file }) => ({
            file,
            folder: INBOX,
            path: file.path,
            ...(attached.at === undefined ? {} : { at: attached.at }),
        }));
    captions.push(...(await savedIn(peers, files, context)));
    return { captions, images };
}

/** A place to sort by: one with none comes last. */
function placeOf(at: number | undefined): number {
    return at ?? Number.POSITIVE_INFINITY;
}

async function savedWhole(
    peers: Peers,
    file: ParcelFile,
    path: string,
    context: Context,
): Promise<Buffer | undefined> {
    const unsaved = (why: unknown) => {
        peers.report(`${file.path} was not saved in the inbox`, why);
        return undefined;
    };
    const bytes = await fetched(peers, file).catch((error: unknown) => error);
    if (!Buffer.isBuffer(bytes)) return unsaved(bytes);
    const wrote = await peers.files.write(path, bytes, context);
    return wrote.ok ? bytes : unsaved(wrote.error);
}

/**
 * Saves each of `files` under its folder, a top name the folder holds
 * already numbered as today's, and gives the captions that say where each
 * landed. A file that is not saved is reported and has no caption.
 */
async function savedIn(
    peers: Peers,
    files: readonly Placed[],
    context: Context,
): Promise<Caption[]> {
    const saved = new Map<string, Saved>();
    const taken = new Map<string, Set<string>>();
    // A path sent twice in one send keeps both, as today's `put_at`.
    const written = new Set<string>();
    const savedAt = async (
        folder: string,
        top: string,
        at: number | undefined,
    ): Promise<Saved> => {
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
            at,
            files: 0,
            bytes: 0,
            folder: false,
        };
        saved.set(key, made);
        return made;
    };
    /** Saves `sent`, or gives why it was not. */
    const save = async (sent: Placed): Promise<unknown> => {
        const [top = "", ...rest] = sent.path.split("/");
        const bytes = await fetched(peers, sent.file).catch(
            (error: unknown) => error,
        );
        if (!Buffer.isBuffer(bytes)) return bytes;
        const at = await savedAt(sent.folder, top, sent.at);
        const names = [...at.path.split("/"), ...rest];
        let path = names.join("/");
        for (let n = 2; written.has(path); n += 1) {
            path = [
                ...names.slice(0, -1),
                numbered(names.at(-1) ?? "", n),
            ].join("/");
        }
        written.add(path);
        const wrote = await peers.files.write(path, bytes, context);
        if (!wrote.ok) return wrote.error;
        at.files += 1;
        at.bytes += bytes.length;
        at.folder ||= rest.length > 0;
        return undefined;
    };
    for (const sent of files) {
        const why = await save(sent);
        if (why !== undefined) {
            peers.report(`${sent.file.path} was not saved in the inbox`, why);
        }
    }
    return [...saved.values()]
        .filter((at) => at.files > 0)
        .map((at, index) => {
            const where = `${peers.files.root}/${at.path}`;
            const size = sizeWords(at.bytes);
            return {
                ...(at.at === undefined ? {} : { at: at.at }),
                words: at.folder
                    ? `[Folder ${index + 1} of this message, ${at.files} files, ${size}, saved at ${where}]`
                    : `[File ${index + 1} of this message, ${size}, saved at ${where}]`,
            };
        });
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
    return `${oneDecimal(size)} ${units[unit]}`;
}

/**
 * `value` to one decimal as Rust's `{:.1}` writes it. Only x.25 and x.75
 * lie exactly half way, and Rust takes the even digit where `toFixed`
 * takes the larger.
 */
function oneDecimal(value: number): string {
    if ((value * 4) % 2 !== 1) return value.toFixed(1);
    const below = Math.floor(value * 10);
    return ((below % 2 === 0 ? below : below + 1) / 10).toFixed(1);
}
