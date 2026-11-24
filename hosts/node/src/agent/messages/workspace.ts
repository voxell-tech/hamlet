/**
 * The workspace's messages: what an agent asks of its workspace, and
 * the answers.
 */
import { Address } from "../../actor/actor/mod.ts";
import type {
    CallId,
    CommandId,
    ConversationRef,
    RealmTask,
    Refused,
} from "./shared.ts";

export interface Stamped {
    readonly call: CallId;
    readonly conversation: ConversationRef;
    readonly realm_task?: RealmTask;
}

export type Program =
    | {
          readonly type: "script";
          readonly script: string;
          readonly network: boolean;
          readonly timeout_ms?: number;
      }
    | {
          readonly type: "tool";
          readonly name: string;
          /** A JSON object, as text. */
          readonly arguments: string;
      };

export interface Run extends Stamped {
    readonly program: Program;
}

export type Need = "network" | "command" | "edit";

export type Exit =
    | { readonly type: "code"; readonly code: number }
    | { readonly type: "timed_out" }
    | { readonly type: "failed"; readonly words: string }
    | { readonly type: "cut_off" }
    | { readonly type: "cancelled" };

export interface Cut {
    readonly total_bytes: number;
    readonly total_lines: number;
    readonly full_output: string;
}

export interface CommandEnd {
    readonly command: CommandId;
    readonly exit: Exit;
    readonly output: string;
    readonly cut?: Cut;
    readonly policy: string;
    readonly took_ms: number;
}

/** Why a call was not run; a wait's end names its reason or lapse. */
export interface NotRunWhy {
    readonly type: string;
    readonly reason?: string;
    readonly lapse?: string;
}

export type RunEnd =
    | ({ readonly type: "ran" } & CommandEnd)
    | {
          readonly type: "not_run";
          readonly why: NotRunWhy;
          readonly words: string;
      };

export type RunAnswer =
    | { readonly type: "ended"; readonly end: RunEnd }
    | { readonly type: "running"; readonly command: CommandId }
    | { readonly type: "waits_for_chief"; readonly needs: readonly Need[] }
    | Refused;

export interface ReadFile {
    readonly path: string;
    readonly offset?: number;
    readonly limit?: number;
}

export interface Search {
    readonly under: string;
    readonly pattern: string;
    readonly name?: string;
    readonly ignore_case: boolean;
}

export interface Find {
    readonly under: string;
    readonly name?: string;
}

export interface Note {
    readonly severity: "info" | "warn";
    readonly code: string;
    readonly message: string;
}

export interface Text {
    readonly text: string;
    readonly notes: readonly Note[];
    readonly truncation?: {
        readonly by?: "lines" | "bytes";
        readonly total_lines: number;
        readonly total_bytes: number;
        readonly output_lines: number;
        readonly output_bytes: number;
        readonly first_line_exceeds_limit: boolean;
    };
}

export type ReadAnswer =
    | ({ readonly type: "text" } & Text)
    | { readonly type: "image"; readonly mime: string; readonly data: string }
    | { readonly type: "failed"; readonly words: string }
    | Refused;

export type SearchAnswer =
    | ({ readonly type: "text" } & Text)
    | { readonly type: "failed"; readonly words: string }
    | Refused;

export interface WriteFile extends Stamped {
    readonly path: string;
    readonly content: string;
}

export interface Replace {
    readonly old: string;
    readonly new: string;
}

export interface EditFile extends Stamped {
    readonly path: string;
    readonly edits: readonly Replace[];
}

export interface Change {
    readonly path: string;
    readonly created: boolean;
    readonly hash: string;
    readonly diff?: {
        readonly diff: string;
        readonly patch: string;
        readonly first_changed_line?: number;
    };
}

export type ChangeEnd =
    | ({ readonly type: "changed" } & Change)
    | { readonly type: "failed"; readonly words: string }
    | {
          readonly type: "not_run";
          readonly why: NotRunWhy;
          readonly words: string;
      };

export type ChangeAnswer =
    | { readonly type: "ended"; readonly end: ChangeEnd }
    | { readonly type: "waits_for_chief"; readonly needs: readonly Need[] }
    | Refused;

export interface AddedTool {
    readonly name: string;
    readonly description: string;
    /** The JSON Schema of its arguments, as text. */
    readonly parameters: string;
}

/** How the agents' commands are confined. */
export type Confinement =
    | { readonly type: "sandbox"; readonly name: string }
    | { readonly type: "insecure" }
    | { readonly type: "remote" };

export interface ToolList {
    readonly tools: readonly AddedTool[];
    readonly faults: readonly {
        readonly folder: string;
        readonly words: string;
    }[];
    readonly confinement: Confinement;
}

export type Which =
    | { readonly type: "all" }
    | { readonly type: "conversation"; readonly conversation: ConversationRef }
    | { readonly type: "one"; readonly call: CallId };

export interface Cancel {
    readonly which: Which;
}

export interface Cancelled {
    readonly commands: readonly CommandId[];
}

export interface WorkspaceActions {
    run(message: Run): RunAnswer;
    read_file(message: ReadFile): ReadAnswer;
    search(message: Search): SearchAnswer;
    find(message: Find): SearchAnswer;
    write_file(message: WriteFile): ChangeAnswer;
    edit_file(message: EditFile): ChangeAnswer;
    list_tools(): ToolList;
    cancel(message: Cancel): Cancelled;
}

export const WORKSPACE: Address<WorkspaceActions> = Address.of(
    "workspace",
    "local",
);
