/** U9 fills this file. Until then: its types, no tools, and an end as JSON text. */
import { defineExtension, type Extension } from "@earendil-works/pi-durable";
import type { Outcome } from "./messages.ts";
import type { Peers } from "./turn.ts";

export interface FileToolArgs {
    read: { path: string; offset?: number; limit?: number };
    write: { path: string; content: string };
    edit: { path: string; edits: { oldText: string; newText: string }[] };
    search: {
        pattern: string;
        path?: string;
        name?: string;
        ignore_case?: boolean;
    };
    find: { path?: string; name?: string };
}

export type FileTool = keyof FileToolArgs;

export function endWords(outcome: Outcome): string {
    return JSON.stringify(outcome);
}

export function workspaceTools(_peers: Peers): Extension {
    return defineExtension({ name: "hamlet.workspace" });
}

export async function handTools(_peers: Peers): Promise<Extension> {
    return defineExtension({ name: "hamlet.hand-tools" });
}
