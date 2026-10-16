import type { Readable, Writable } from "node:stream";

/** Streams owned by the caller for one command invocation. */
export type CommandIO = {
    stdin: Readable & { isTTY?: boolean };
    stdout: Writable;
    stderr: Writable;
};
