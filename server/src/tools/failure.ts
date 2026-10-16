import type { Home } from "../home.ts";

/** A search or listing refused before any file is changed. */
export type SearchFailure =
    | ({
          operation: "search" | "find";
          retrySafe: true;
      } & (
          | { kind: "pattern-empty" }
          | { kind: "pattern-not-linear" }
          | { kind: "pattern-invalid"; reason: string }
          | { kind: "not-a-folder"; path: string }
          | { kind: "unreadable"; path: string; reason: string }
      ))
    | {
          kind: "no-environment";
          operation: "search" | "find" | "run-tool";
          retrySafe: true;
      };

/** A command that could not start or whose effects need inspection before retrying. */
export type RunFailure = {
    kind: "run-refused";
    operation: "run-tool";
    retrySafe: false;
    reason: string;
};

/** Expected failures rendered for an agent's tools. */
export type ToolFailure = SearchFailure | RunFailure;

/** Agent-facing failure words with a permitted next action. */
export function toolFailureText(failure: ToolFailure): string {
    switch (failure.kind) {
        case "run-refused":
            return `Error: ${failure.reason}. Inspect the output before trying again, or tell the chief.`;
        case "pattern-empty":
            return "`pattern` is required. Supply a regular expression.";
        case "pattern-not-linear":
            return "`pattern` holds a backreference, a lookaround or another part a search in linear time cannot take. Use a linear-time pattern.";
        case "pattern-invalid":
            return `\`pattern\` is not a regular expression: ${failure.reason}. Correct the pattern.`;
        case "not-a-folder":
            return `${failure.path} is not a folder. Choose a folder, or use search to read a file.`;
        case "unreadable":
            return `${failure.path} could not be read (${failure.reason}). Choose a readable path within your reach, or tell the chief.`;
        case "no-environment":
            return "No execution environment is configured. Tell the chief before trying again.";
    }
    const unhandled: never = failure;
    throw new Error(`Unclassified tool failure: ${unhandled}`);
}

/** Reports a private tool defect and throws a model-safe error retaining its cause. */
export function toolDefect(home: Home, tool: string, cause: unknown): never {
    console.error(
        home.secrets.words(
            `Tool ${tool} failed: ${cause instanceof Error ? (cause.stack ?? cause.message) : String(cause)}`,
        ),
    );
    throw new Error(
        `The ${tool} tool failed. Inspect its output and tell the chief before trying again.`,
        { cause },
    );
}
