/**
 * A merge's move of the absorbed hire's `inbox/` into the kept hire's:
 * the absorbed hire's agent reads it in parts, the kept hire's writes each
 * file under `inbox/from-hire-<n>/`, and the absorbed one drops what was
 * taken.
 */
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { RawAccess } from "../actor/db/mod.ts";
import { type AgentCall, generationOf } from "./actor.ts";
import { cost, MOST_PART } from "./branch.ts";
import type { AgentFilesystem } from "./filesystem.ts";
import type {
    DropInbox,
    InboxPart,
    ReadInbox,
    TakeInbox,
} from "./messages/agent.ts";
import type { Piece } from "./messages/profile.ts";

const INBOX = "inbox";
const context = BACKGROUND_CONTEXT;

/** A file's pieces taken before its last, which may come in a later part. */
export async function migrateInbox(
    raw: Pick<RawAccess, "execute">,
): Promise<void> {
    await raw.execute(`
        CREATE TABLE IF NOT EXISTS inbox_piece (
            from_hire INTEGER NOT NULL,
            path TEXT NOT NULL,
            at INTEGER NOT NULL,
            bytes BLOB NOT NULL,
            PRIMARY KEY (from_hire, path, at)
        ) STRICT
    `);
}

export async function read_inbox(
    c: AgentCall,
    { after }: ReadInbox,
): Promise<InboxPart> {
    const files = generationOf(c).peers.files;
    const pieces: Piece[] = [];
    let size = 0;
    for (const file of await waiting(files, INBOX)) {
        const path = file.path.slice(INBOX.length + 1);
        if (after !== undefined && path < after.path) continue;
        const read = await files.read(file.path, context);
        if (!read.ok) throw read.error;
        let offset = path === after?.path ? after.offset : 0;
        for (;;) {
            const bytes = read.value.subarray(
                offset,
                offset + MOST_PART - cost(path, 0),
            );
            const more = cost(path, bytes.length);
            if (pieces.length > 0 && size + more > MOST_PART) {
                return { files: pieces, next: { path, offset } };
            }
            const last = offset + bytes.length >= read.value.length;
            pieces.push({
                path,
                offset,
                bytes: Buffer.from(bytes).toString("base64"),
                last,
                ...(file.executable ? { executable: true } : {}),
            });
            size += more;
            offset += bytes.length;
            if (last) break;
        }
    }
    return { files: pieces };
}

export async function take_inbox(
    c: AgentCall,
    { from, files: pieces }: TakeInbox,
): Promise<void> {
    const files = generationOf(c).peers.files;
    for (const piece of pieces) {
        const whole = await c.db.transaction(async (db) => {
            const ofFile = [from, piece.path] as const;
            await db.run(
                "DELETE FROM inbox_piece WHERE from_hire = ? AND path = ? AND at >= ?",
                ...ofFile,
                piece.offset,
            );
            const held =
                (
                    await db.get<{ held: number }>(
                        "SELECT coalesce(sum(length(bytes)), 0) AS held FROM inbox_piece WHERE from_hire = ? AND path = ?",
                        ...ofFile,
                    )
                )?.held ?? 0;
            // Answering would let the realm drop a file not written.
            if (held !== piece.offset) {
                throw new Error(
                    `inbox/${piece.path} of hire#${from} has ${held} bytes before a piece at ${piece.offset}`,
                );
            }
            const bytes = Buffer.from(piece.bytes, "base64");
            if (!piece.last) {
                await db.run(
                    "INSERT INTO inbox_piece (from_hire, path, at, bytes) VALUES (?, ?, ?, ?)",
                    ...ofFile,
                    piece.offset,
                    bytes,
                );
                return undefined;
            }
            const before: Buffer[] = [];
            const at = await db.all<{ at: number }>(
                "SELECT at FROM inbox_piece WHERE from_hire = ? AND path = ? ORDER BY at",
                ...ofFile,
            );
            for (const row of at) {
                // One piece at a time: the native host reads a blob back
                // as an array of numbers.
                const stored = await db.get<{ bytes: Uint8Array | number[] }>(
                    "SELECT bytes FROM inbox_piece WHERE from_hire = ? AND path = ? AND at = ?",
                    ...ofFile,
                    row.at,
                );
                if (stored !== undefined)
                    before.push(Buffer.from(stored.bytes));
            }
            return Buffer.concat([...before, bytes]);
        });
        if (whole === undefined) continue;
        const wrote = await files.write(
            `${INBOX}/from-hire-${from}/${piece.path}`,
            whole,
            context,
            piece.executable ?? false,
        );
        if (!wrote.ok) throw wrote.error;
        await c.db.run(
            "DELETE FROM inbox_piece WHERE from_hire = ? AND path = ?",
            from,
            piece.path,
        );
    }
}

export async function drop_inbox(
    c: AgentCall,
    { paths }: DropInbox,
): Promise<void> {
    const files = generationOf(c).peers.files;
    for (const path of paths) {
        const removed = await files.remove(`${INBOX}/${path}`, context);
        if (!removed.ok && removed.error.code !== "not_found") {
            throw removed.error;
        }
    }
}

/**
 * Every file at or below `folder`, in path order. A walk is not enough: it
 * stops after `LOOKED` names and skips folders such as `node_modules`,
 * which a folder sent to the agent may hold.
 */
async function waiting(
    files: AgentFilesystem,
    folder: string,
): Promise<{ readonly path: string; readonly executable: boolean }[]> {
    const found: { path: string; executable: boolean }[] = [];
    const under = async (folder: string): Promise<void> => {
        const listed = await files.list(folder, context);
        if (!listed.ok) {
            if (listed.error.code === "not_found") return;
            throw listed.error;
        }
        for (const entry of listed.value) {
            const path = `${folder}/${entry.name}`;
            if (entry.kind === "directory") await under(path);
            else if (entry.kind === "file") {
                found.push({ path, executable: entry.executable === true });
            }
        }
    };
    await under(folder);
    return found.sort((a, b) => (a.path < b.path ? -1 : 1));
}
