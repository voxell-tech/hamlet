/**
 * A profile's messages: what an agent asks of its profile, and how the
 * agent reads the answers it keeps.
 */
import {
    arr,
    bool,
    lit,
    obj,
    opt,
    type Static,
    str,
    union,
} from "@earendil-works/pi-schema";
import { Address } from "../../actor/actor/mod.ts";
import {
    type CallId,
    OPEN,
    type ProfileId,
    type Refused,
    reader,
    U32,
} from "./shared.ts";

/** A commit of a profile's repository: its SHA-1, as 40 hex digits. */
export type CommitId = string;

export interface Path {
    readonly path: string;
}

/** A commit as a model names it. */
export type Revision =
    | { readonly type: "main" }
    | { readonly type: "version"; readonly version: string }
    | { readonly type: "commit"; readonly commit: CommitId };

/** The character at a profile's tip, which the agent keeps when its version is newer. */
export interface Character {
    readonly version: number;
    readonly text: string;
}

/**
 * A file, or the bytes of one from `offset` when it is too large for one
 * part: a whole file is one piece at 0 that is `last`.
 */
export interface Piece {
    readonly path: string;
    readonly offset: number;
    /** Base64. */
    readonly bytes: string;
    readonly last: boolean;
    readonly executable?: boolean;
}

/** Where the next part of a fill starts, in the commit its first part read. */
export interface FileCursor {
    readonly commit: CommitId;
    readonly file: number;
    readonly offset: number;
}

/** One part of every file at the branch's tip, or of what changed since `since`. */
export interface Files {
    readonly since?: CommitId;
    readonly after?: FileCursor;
}

export interface FilesPart {
    readonly type: "part";
    readonly tip: CommitId;
    readonly files: readonly Piece[];
    readonly removed: readonly Path[];
    /** Absent on the last part. */
    readonly next?: FileCursor;
    /** On the first part. */
    readonly character?: Character;
}

export type FilesAnswer = FilesPart | Refused;

export type FileChange =
    | ({ readonly type: "write" } & Piece)
    | { readonly type: "remove"; readonly path: string };

/** One part of what the agent's filesystem changed since `base`. */
export interface Save {
    readonly call: CallId;
    readonly base: CommitId;
    readonly part: number;
    readonly last: boolean;
    readonly changes: readonly FileChange[];
}

export type SaveAnswer =
    | { readonly type: "saved"; readonly tip: CommitId }
    | { readonly type: "kept" }
    | Refused;

export interface ProfileActions {
    files(message: Files): FilesAnswer;
    save(message: Save): SaveAnswer;
}

export function profileAt(profile: ProfileId): Address<ProfileActions> {
    return Address.of("profile", String(profile));
}

/** The profile the agent was filled from, whichever it is. */
export const OWN_PROFILE = "own profile";

export const CHARACTER = obj({ version: U32, text: str() }, OPEN);

const PIECE = obj(
    {
        path: str(),
        offset: U32,
        bytes: str(),
        last: bool(),
        executable: opt(bool()),
    },
    OPEN,
);

const CURSOR = obj({ commit: str(), file: U32, offset: U32 }, OPEN);

export function characterOf({
    version,
    text,
}: Static<typeof CHARACTER>): Character {
    return { version, text };
}

function pieceOf({
    path,
    offset,
    bytes,
    last,
    executable,
}: Static<typeof PIECE>): Piece {
    return {
        path,
        offset,
        bytes,
        last,
        ...(executable === undefined ? {} : { executable }),
    };
}

/** The profile's answer to `files`, as the agent keeps it. */
export const readFiles = reader(
    union([
        obj(
            {
                type: lit("part"),
                tip: str(),
                files: arr(PIECE),
                removed: arr(obj({ path: str() }, OPEN)),
                next: opt(CURSOR),
                character: opt(CHARACTER),
            },
            OPEN,
        ),
        obj(
            {
                type: lit("refused"),
                why: obj({ type: str() }, OPEN),
                words: str(),
            },
            OPEN,
        ),
    ]),
    (answer): FilesAnswer =>
        answer.type === "refused"
            ? {
                  type: "refused",
                  why: { type: answer.why.type },
                  words: answer.words,
              }
            : {
                  type: "part",
                  tip: answer.tip,
                  files: answer.files.map(pieceOf),
                  removed: answer.removed.map(({ path }) => ({ path })),
                  ...(answer.next === undefined
                      ? {}
                      : {
                            next: {
                                commit: answer.next.commit,
                                file: answer.next.file,
                                offset: answer.next.offset,
                            },
                        }),
                  ...(answer.character === undefined
                      ? {}
                      : { character: characterOf(answer.character) }),
              },
);
