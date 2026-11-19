/**
 * The agent's filesystem and its branch of the profile it was filled from.
 * A fill writes every file at the branch's tip; after a turn, a save sends
 * what the filesystem changed since the commit it holds, then pulls what
 * the branch holds now; a move pulls what changed on the branch. The
 * profile never sends and never takes `CHARACTER.md` or `inbox/`.
 *
 * `filed` holds each file's digest as the commit in `agent.filled` holds
 * it, as far as the agent wrote, sent or pulled it, so a save sends only
 * what differs and a pull writes over no file the agent changed. One piece
 * of this work runs at a time.
 */
import { createHash, randomUUID } from "node:crypto";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type { ConversationId } from "@earendil-works/pi-durable";
import type { RawAccess } from "../actor/db/mod.ts";
import type { AgentContext } from "./actor.ts";
import { keep } from "./copies.ts";
import { type AgentFilesystem, LOOKED } from "./filesystem.ts";
import {
    type CallId,
    type CommitId,
    type FileChange,
    type FileCursor,
    type ProfileId,
    type Removal,
    readAnswer,
} from "./messages.ts";
import { type Peers, profileCalls } from "./turn.ts";

/** As `MOST_PART` and `PIECE_FRAME` of `crates/hamlet_messages/src/lib.rs`. */
const MOST_PART = 720 * 1024;
const PIECE_FRAME = 64;

export async function migrateBranch(
    raw: Pick<RawAccess, "execute">,
): Promise<void> {
    await raw.execute(`
        CREATE TABLE IF NOT EXISTS filed (
            path TEXT PRIMARY KEY,
            digest TEXT NOT NULL
        ) STRICT
    `);
}

export interface Branch {
    /** Fills an agent filled from no profile yet; a repeat writes nothing. */
    fill(profile: ProfileId): Promise<void>;
    /**
     * Pulls the branch's tip between turns, once the work under way has
     * settled. It returns at once: a profile drops a call answered late.
     */
    moved(): void;
    /** Removes `path` now, or once no turn or other work runs. */
    remove(path: string): Promise<Removal>;
    /** Saves, and does what waited for the turns to end, once none runs. */
    turnEnded(conversation: ConversationId): Promise<void>;
}

/** Whether a turn runs in a conversation other than `except`. */
export type Busy = (except?: ConversationId) => Promise<boolean>;

function withheld(path: string): boolean {
    return (
        path === "CHARACTER.md" || path === "inbox" || path.startsWith("inbox/")
    );
}

function digestOf(bytes: Uint8Array): string {
    return createHash("sha256").update(bytes).digest("hex");
}

function cost(path: string, bytes: number): number {
    return Buffer.byteLength(path) + bytes + PIECE_FRAME;
}

/**
 * `changes` in parts each of at most `MOST_PART`, a file too large for one
 * part in pieces at their offsets.
 */
export function inParts(changes: readonly Change[]): FileChange[][] {
    const parts: FileChange[][] = [];
    let part: FileChange[] = [];
    let size = 0;
    const add = (change: FileChange, bytes: number) => {
        const more = cost(change.path, bytes);
        if (part.length > 0 && size + more > MOST_PART) {
            parts.push(part);
            part = [];
            size = 0;
        }
        part.push(change);
        size += more;
    };
    for (const change of changes) {
        if (change.bytes === undefined) {
            add({ type: "remove", path: change.path }, 0);
            continue;
        }
        const room = MOST_PART - cost(change.path, 0);
        let offset = 0;
        do {
            const piece = change.bytes.subarray(offset, offset + room);
            const last = offset + piece.length >= change.bytes.length;
            add(
                {
                    type: "write",
                    path: change.path,
                    offset,
                    bytes: Buffer.from(piece).toString("base64"),
                    last,
                },
                piece.length,
            );
            offset += piece.length;
        } while (offset < change.bytes.length);
    }
    if (part.length > 0) parts.push(part);
    return parts;
}

/** A file to write, or one to remove when it has no `bytes`. */
export interface Change {
    readonly path: string;
    readonly bytes?: Uint8Array;
}

export function branchOf(
    c: Pick<AgentContext, "db">,
    peers: Peers,
    busy: Busy,
): Branch {
    const files: AgentFilesystem = peers.files;
    const context = BACKGROUND_CONTEXT;
    let moved = false;
    /** A save sent and not answered yet, its parts before `next` answered. */
    let unanswered:
        | {
              readonly profile: ProfileId;
              readonly base: CommitId;
              readonly call: CallId;
              readonly changes: readonly Change[];
              readonly parts: readonly FileChange[][];
              next: number;
          }
        | undefined;
    const removals = new Set<string>();
    let last: Promise<unknown> = Promise.resolve();
    let working = 0;
    const oneAtATime = <T>(work: () => Promise<T>): Promise<T> => {
        working += 1;
        const done = last.then(work).finally(() => {
            working -= 1;
        });
        last = done.catch(() => undefined);
        return done;
    };

    const held = async () =>
        c.db.get<{ profile: ProfileId | null; filled: CommitId | null }>(
            "SELECT profile, filled FROM agent WHERE id = 1",
        );

    const forget = (path: string) =>
        c.db.run(
            "DELETE FROM filed WHERE path = ? OR substr(path, 1, length(?) + 1) = ? || '/'",
            path,
            path,
            path,
        );

    const removeNow = async (path: string): Promise<void> => {
        const removed = await files.remove(path, context);
        if (!removed.ok && removed.error.code !== "not_found") {
            throw removed.error;
        }
        await forget(path);
    };

    const file = (path: string, digest: string) =>
        c.db.run(
            "INSERT INTO filed (path, digest) VALUES (?, ?) ON CONFLICT (path) DO UPDATE SET digest = excluded.digest",
            path,
            digest,
        );

    /**
     * Whether the file at `path` is as `filed` holds it, so a pull may
     * write over it or remove it.
     */
    const unchanged = async (path: string): Promise<boolean> => {
        const kept = await c.db.get<{ digest: string }>(
            "SELECT digest FROM filed WHERE path = ?",
            path,
        );
        const read = await files.read(path, context);
        if (!read.ok) {
            if (read.error.code !== "not_found") throw read.error;
            return kept === undefined;
        }
        return kept?.digest === digestOf(read.value);
    };

    /** Removes `path` for a pull, or only forgets it if the agent changed it. */
    const pullRemoval = async (path: string): Promise<void> => {
        if (await unchanged(path)) await removeNow(path);
        else await forget(path);
    };

    /**
     * Writes what changed on the branch since `since`, or its whole tip. A
     * file the agent changed since `filed` stays as the agent has it, so the
     * next save sends it on the tip.
     */
    const pull = async (
        profile: ProfileId,
        since?: CommitId,
    ): Promise<void> => {
        const calls = profileCalls(peers, profile);
        const atTip = new Set<string>();
        const pieces = new Map<string, Buffer[]>();
        let after: FileCursor | undefined;
        let tip: CommitId | undefined;
        do {
            const answer = readAnswer.files(
                await calls.files({
                    ...(since === undefined ? {} : { since }),
                    ...(after === undefined ? {} : { after }),
                }),
            );
            if (answer.type === "refused") {
                // A purge rewrote the commit the agent holds: it fills again
                // from nothing.
                if (since !== undefined && answer.why.type === "commit_gone") {
                    return pull(profile);
                }
                throw new Error(answer.words);
            }
            if (after === undefined && answer.character !== undefined) {
                const { version, text } = answer.character;
                await keep(c, { character: { version, value: text } });
            }
            for (const { path } of answer.removed) {
                await pullRemoval(path).catch((error: unknown) =>
                    peers.report(`${path} was not removed for a pull`, error),
                );
            }
            for (const piece of answer.files) {
                const bytes = Buffer.from(piece.bytes, "base64");
                const before =
                    piece.offset === 0 ? [] : (pieces.get(piece.path) ?? []);
                if (!piece.last) {
                    pieces.set(piece.path, [...before, bytes]);
                    continue;
                }
                pieces.delete(piece.path);
                const whole = Buffer.concat([...before, bytes]);
                if (await unchanged(piece.path)) {
                    const wrote = await files.write(piece.path, whole, context);
                    if (!wrote.ok) throw wrote.error;
                }
                await file(piece.path, digestOf(whole));
                atTip.add(piece.path);
            }
            tip = answer.tip;
            after = answer.next;
        } while (after !== undefined);
        if (since === undefined) {
            const kept = await c.db.all<{ path: string }>(
                "SELECT path FROM filed",
            );
            for (const { path } of kept) {
                if (atTip.has(path)) continue;
                await pullRemoval(path).catch((error: unknown) =>
                    peers.report(`${path} was not removed for a fill`, error),
                );
            }
        }
        await c.db.run("UPDATE agent SET filled = ? WHERE id = 1", tip ?? null);
    };

    /** What the filesystem changed since the commit it holds. */
    const changed = async (): Promise<Change[]> => {
        const walked = await files.walk("", context);
        if (!walked.ok) throw walked.error;
        if (walked.value.capped) {
            peers.report(
                "the filesystem was saved in part",
                new Error(
                    `the walk stopped after ${LOOKED} names, so files past them were not saved`,
                ),
            );
        }
        const kept = new Map(
            (
                await c.db.all<{ path: string; digest: string }>(
                    "SELECT path, digest FROM filed",
                )
            ).map(({ path, digest }) => [path, digest]),
        );
        const changes: Change[] = [];
        for (const path of walked.value.files) {
            if (withheld(path)) continue;
            const read = await files.read(path, context);
            if (!read.ok) continue;
            if (kept.get(path) !== digestOf(read.value)) {
                changes.push({ path, bytes: read.value });
            }
            kept.delete(path);
        }
        // A file the walk does not go into is not gone.
        for (const path of kept.keys()) {
            const read = await files.read(path, context);
            if (!read.ok && read.error.code === "not_found")
                changes.push({ path });
        }
        return changes;
    };

    /**
     * Sends what changed since the commit the filesystem holds, or, while a
     * save is unanswered, that save again under its call from the part whose
     * answer was lost, then saves what changed since.
     */
    const save = async (): Promise<void> => {
        const resent = unanswered !== undefined;
        if (unanswered === undefined) {
            const row = await held();
            if (row?.profile == null || row.filled === null) return;
            const changes = await changed();
            if (changes.length === 0) return;
            unanswered = {
                profile: row.profile,
                base: row.filled,
                call: randomUUID() as CallId,
                changes,
                parts: inParts(changes),
                next: 0,
            };
        }
        const sending = unanswered;
        const calls = profileCalls(peers, sending.profile);
        for (const [part, changes] of sending.parts.entries()) {
            if (part < sending.next) continue;
            const answer = await calls.save({
                call: sending.call,
                base: sending.base,
                part,
                last: part === sending.parts.length - 1,
                changes,
            });
            if (answer.type === "refused") {
                unanswered = undefined;
                if (answer.why.type === "commit_gone") {
                    return pull(sending.profile);
                }
                throw new Error(answer.words);
            }
            sending.next = part + 1;
        }
        unanswered = undefined;
        for (const change of sending.changes) {
            if (change.bytes === undefined) await forget(change.path);
            else await file(change.path, digestOf(change.bytes));
        }
        await pull(sending.profile, sending.base);
        if (resent) await save();
    };

    const removeWaiting = async (): Promise<void> => {
        for (const path of removals) {
            removals.delete(path);
            await removeNow(path).catch((error: unknown) =>
                peers.report(`${path} was not removed after a turn`, error),
            );
        }
    };

    /** Removes what waited, saves, and pulls a move that waited. */
    const settle = () =>
        oneAtATime(async () => {
            await removeWaiting();
            await save();
            if (!moved) return;
            moved = false;
            const row = await held();
            if (row?.profile == null || row.filled === null) return;
            await pull(row.profile, row.filled);
        });

    return {
        fill: (profile) =>
            oneAtATime(async () => {
                if ((await held())?.filled != null) return;
                await c.db.run(
                    "UPDATE agent SET profile = ? WHERE id = 1",
                    profile,
                );
                await pull(profile);
            }),
        moved: () => {
            moved = true;
            void (async () => {
                if (!(await busy())) await settle();
            })().catch((error: unknown) =>
                peers.report("the branch's move was not pulled", error),
            );
        },
        remove: async (path) => {
            if (working === 0 && !(await busy())) {
                await oneAtATime(() => removeNow(path));
                return "removed";
            }
            removals.add(path);
            // Work under way may be the end of the last turn, which removed
            // what waited before it began.
            if (working > 0) void oneAtATime(removeWaiting);
            return "later";
        },
        turnEnded: async (conversation) => {
            if (await busy(conversation)) return;
            await settle();
        },
    };
}
