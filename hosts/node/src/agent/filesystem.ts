import type { Context } from "@earendil-works/chord/context";
import type {
    FileError,
    FileInfo,
    Result,
} from "@earendil-works/pi-durable/env";

/** The most names a search or a find looks at. */
export const LOOKED = 20_000;

/** The most matches or files a search or a find gives. */
export const FOUND = 100;

/** A name in a folder of the agent's filesystem, a link itself rather than what it names. */
export type FileEntry = Omit<FileInfo, "mtimeMs"> & {
    /** A file that runs, where the filesystem keeps that. */
    readonly executable?: boolean;
};

export interface Found<T> {
    readonly found: T[];
    /** More than `FOUND` matched. */
    readonly more: boolean;
    /** It looked at `LOOKED` names and stopped with fewer than `FOUND`. */
    readonly capped: boolean;
}

/** A line that matched a search, numbered from 1. */
export interface Match {
    readonly path: string;
    readonly line: number;
    /** The line as a search shows it: trimmed, and cut at 300 characters with `...`. */
    readonly text: string;
}

/** One replacement in a file: `old` occurs once in it, and becomes `new`. */
export interface Edit {
    readonly old: string;
    readonly new: string;
}

/**
 * The agent's filesystem: its own folder, which no other agent and no
 * project shares. Each path is relative to the folder or absolute below it.
 * No operation follows a link, nor a path out of the folder, also while a
 * command of the agent swaps one of its folders for a link. The operations
 * keep the shapes of pi-durable's `ExecutionEnv`: an expected failure is
 * returned as a `FileError`, and each takes the call's `Context`.
 */
export interface AgentFilesystem {
    /** As pi-durable's `FileSystem.id`: equal ids see the same files. */
    readonly id: string;
    /** The folder's absolute path, with `/` between names. */
    readonly root: string;
    read(
        path: string,
        context: Context,
    ): Promise<Result<Uint8Array, FileError>>;
    /**
     * Writes the file whole, making its folders, and answers whether it
     * made the file. A link there is replaced, not written through. With
     * `executable`, the file runs or does not; without it, it keeps the
     * mode of the file it replaces.
     */
    write(
        path: string,
        content: string | Uint8Array,
        context: Context,
        executable?: boolean,
    ): Promise<Result<boolean, FileError>>;
    /** Makes every edit of `edits` in the UTF-8 text of the file at once. */
    edit(
        path: string,
        edits: readonly Edit[],
        context: Context,
    ): Promise<
        Result<{ readonly before: string; readonly after: string }, FileError>
    >;
    /** The names in one folder, sorted. */
    list(
        path: string,
        context: Context,
    ): Promise<Result<FileEntry[], FileError>>;
    /**
     * The lines that match `pattern`, a regular expression that runs in
     * linear time, in the text files at or below `path`, and only in those
     * `options.name` matches as `find` reads a glob. It goes into no folder
     * a walk skips below `path`.
     */
    search(
        pattern: string,
        path: string,
        options: { ignoreCase?: boolean; name?: string } | undefined,
        context: Context,
    ): Promise<Result<Found<Match>, FileError>>;
    /**
     * The files at or below `path` whose name `glob` matches, or whose path
     * from `path` it matches when it holds a `/`. In `glob`, `*` stands for
     * any run of characters and `?` for one. It goes into no folder a walk
     * skips below `path`.
     */
    find(
        glob: string,
        path: string,
        context: Context,
    ): Promise<Result<Found<string>, FileError>>;
    /**
     * Every file at or below `path`, each from the folder, after at most
     * `LOOKED` names looked at, and those of them that run where the
     * filesystem keeps that. It goes into no folder a walk skips below
     * `path`.
     */
    walk(
        path: string,
        context: Context,
    ): Promise<
        Result<
            {
                readonly files: string[];
                readonly capped: boolean;
                readonly executable?: readonly string[];
            },
            FileError
        >
    >;
    /** Removes the file, or the folder with all it holds. */
    remove(path: string, context: Context): Promise<Result<void, FileError>>;
    /** Copies a file to another place in the agent's filesystem. */
    copy(
        from: string,
        to: string,
        context: Context,
    ): Promise<Result<void, FileError>>;
}
