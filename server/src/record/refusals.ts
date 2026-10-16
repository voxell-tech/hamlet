import type { Result } from "../result.ts";
import { err } from "../result.ts";

/** A refused record change, before any membership or road change is made. */
export type RecordRefusal = {
    operation: string;
    hamlet: string;
    retrySafe: true;
} & (
    | { kind: "agent-missing"; name: string; local?: boolean }
    | { kind: "agent-departed"; farewell: string }
    | { kind: "name-format" }
    | { kind: "name-reserved"; name: string }
    | { kind: "name-taken"; name: string; archive?: string }
    | { kind: "hamlet-full"; most: number }
    | { kind: "not-mayor" }
    | { kind: "proposed-mayor-missing" }
    | { kind: "already-mayor"; name: string }
    | { kind: "road-budget"; text: string }
    | { kind: "mayor-untouchable" }
    | {
          kind: "protected";
          name: string;
          chief: string;
          text: string;
          done: number | null;
      }
    | { kind: "purpose-empty" | "reason-empty" }
    | { kind: "archive-missing"; archive: string }
    | { kind: "merge-unfinished" }
    | { kind: "kept-departed"; kept: string; absorbed: string }
    | { kind: "later-merge"; later: string; kept: string; absorbed: string }
    | { kind: "road-exists" | "road-missing"; a: string; b: string }
    | { kind: "roads-full"; most: number }
    | { kind: "office-road" | "road-self" }
);

/** Adds the operation and hamlet to a classified record refusal. */
export function refusal<
    T extends Omit<RecordRefusal, "operation" | "hamlet" | "retrySafe">,
>(
    operation: string,
    hamlet: string,
    reason: T,
): Result<never, T & { operation: string; hamlet: string; retrySafe: true }> {
    return err({ ...reason, operation, hamlet, retrySafe: true });
}

/** Audience-specific record wording; chief receipts preserve their command text. */
export function recordRefusalText(
    error: RecordRefusal,
    audience: "chief" | "agent",
): string {
    switch (error.kind) {
        case "agent-missing":
            return `no agent named \`${error.name}\`${error.local ? " in this hamlet" : ""}`;
        case "agent-departed":
            return error.farewell;
        case "name-format":
            return "a name is 1 to 24 lowercase letters and hyphens, such as `librarian`";
        case "name-reserved":
            return `\`${error.name}\` is reserved`;
        case "name-taken":
            return error.archive
                ? `an agent named \`${error.name}\` lives in the hamlet now. Restore this one under another name: /restore ${error.archive} as <new-name>`
                : `an agent named \`${error.name}\` already exists`;
        case "hamlet-full":
            return error.operation === "restore"
                ? `the hamlet is full (${error.most} agents); remove one first, or raise mostAgents in the limits`
                : `the hamlet is full (${error.most} agents). Only the chief can raise mostAgents on the hamlet's Bylaws page`;
        case "road-budget":
            return error.text;
        case "proposed-mayor-missing":
            return "The proposed mayor no longer lives in this hamlet.";
        case "already-mayor":
            return `\`${error.name}\` is the mayor already`;
        case "not-mayor":
            return "Only the mayor may do that.";
        case "mayor-untouchable":
            return "the mayor cannot be removed or merged; replacing a mayor takes a chief or a vote";
        case "protected":
            return `\`${error.name}\` has a task from the chief ${error.chief} ("${Array.from(error.text).slice(0, 80).join("")}") that ${error.done === null ? "is not finished" : "has only just ended"}. No agent may be removed or merged to avoid a chief's task. Wait until the chief has had the answer, or ask the chief to do this.`;
        case "purpose-empty":
            return "say what the agent is for in `purpose`";
        case "reason-empty":
            return "give a `reason`; it is kept in the record";
        case "archive-missing":
            return `nothing is archived under \`${error.archive}\`; /record lists what is`;
        case "merge-unfinished":
            return "The merge has not finished yet; try again after it finishes.";
        case "kept-departed":
            return `\`${error.kept}\`, which \`${error.absorbed}\` was merged into, has left the hamlet itself; restore that first`;
        case "later-merge":
            return `\`${error.later}\` was merged into \`${error.kept}\` after \`${error.absorbed}\`; split that one first`;
        case "office-road":
            return error.operation === "take-up-road"
                ? "an agent's road to the mayor stays for as long as the agent does"
                : "every agent already has a road to the mayor";
        case "road-self":
            return "a road needs two different agents";
        case "road-exists":
            return `\`${error.a}\` and \`${error.b}\` already have a road`;
        case "road-missing":
            return `\`${error.a}\` and \`${error.b}\` have no road`;
        case "roads-full":
            return `the hamlet has all the roads ${audience === "chief" ? "your limits allow" : "the chief allows"} (${error.most}). Only the chief can raise mostRoads on the hamlet's Bylaws page`;
    }
    const unhandled: never = error;
    throw new Error(`Unclassified record refusal: ${unhandled}`);
}
