import type { Readable, Writable } from "node:stream";

/** Streams owned by the caller for one command invocation. */
export type CommandIO = {
    /** Real command children inherit descriptors without Node reading stdin. */
    childStdio?: "inherit";
    stdin: Readable & { isTTY?: boolean };
    stdout: Writable;
    stderr: Writable;
};
