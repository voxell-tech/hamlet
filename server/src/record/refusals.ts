import type { Result } from "../result.ts";
import { err } from "../result.ts";
import type { AgentRef } from "./document.ts";

/** A refused record change, before any membership or road change is made. */
export type RecordRefusal = {
    operation: string;
    hamlet: string;
    retrySafe: true;
} & RecordReason;

type RecordReason =
    | { kind: "agent-missing"; name: string; local?: boolean }
    | { kind: "agent-departed"; farewell: string }
    | { kind: "name-format" }
    | { kind: "name-reserved"; name: string }
    | { kind: "name-taken"; name: string; archive?: string }
    | { kind: "hamlet-full"; most: number }
    | { kind: "not-mayor" }
    | {
          kind:
              | "vote-chief"
              | "vote-nonmember"
              | "vote-reason"
              | "vote-hamlet-missing"
              | "vote-overturn-chief"
              | "vote-mayor-chief";
      }
    | { kind: "vote-already-open"; vote: number; motion: string }
    | {
          kind:
              | "vote-retry"
              | "vote-overturned"
              | "vote-not-passed"
              | "vote-mayor-changed";
          vote: number;
      }
    | { kind: "vote-too-few"; least: number; actual: number }
    | { kind: "vote-candidate"; name: string }
    | { kind: "vote-missing"; vote: string }
    | {
          kind:
              | "ballot-not-asked"
              | "ballot-choice"
              | "ballot-reason"
              | "ballot-ended"
              | "ballot-departed";
      }
    | {
          kind: "ballot-closed" | "ballot-not-voter" | "ballot-already-cast";
          vote: number;
      }
    | {
          kind:
              | "hamlet-title-long"
              | "hamlet-position"
              | "hamlet-purpose"
              | "hamlet-used"
              | "first-hamlet"
              | "house-move-pending";
      }
    | { kind: "record-hamlet-missing"; removed: boolean }
    | { kind: "record-hamlet-chief"; verb: string }
    | { kind: "hamlets-full"; actual?: number; most?: number }
    | { kind: "hamlet-restore-name" | "founder-taken"; name: string }
    | {
          kind:
              | "agreement-mayor"
              | "realm-roads-closed"
              | "realm-road-office"
              | "road-agreed"
              | "move-pending"
              | "move-mayor";
      }
    | { kind: "realm-road-local" | "realm-road-neither"; a: string; b: string }
    | { kind: "move-already-there" | "move-neither"; name: string }
    | { kind: "merge-too-large" | "merge-path" }
    | { kind: "merge-path-taken"; path: string }
    | {
          kind: "unsplit-merge";
          agent: AgentRef;
          name: string;
          absorbed: string;
          archive: string;
          recovery: "split-or-erase";
      }
    | { kind: "record-chief" }
    | {
          kind:
              | "merge-self"
              | "merged-purpose-empty"
              | "proposed-kept-missing"
              | "merge-changed";
      }
    | { kind: "record-usage"; command: "road" | "unroad" | "merge" }
    | { kind: "erase-missing"; archive: string }
    | { kind: "model-pinned"; name: string }
    | { kind: "model-not-allowed"; model: string; models: readonly string[] }
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
    | { kind: "office-road" | "road-self" };

/** Adds the operation and hamlet to a classified record refusal. */
export function refusal<const T extends RecordReason>(
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
            if (error.operation === "move")
                return `the hamlet \`${error.hamlet}\` is full`;
            return error.operation === "restore"
                ? `the hamlet is full (${error.most} agents); remove one first, or raise mostAgents in the limits`
                : `the hamlet is full (${error.most} agents). Only the chief can raise mostAgents on the hamlet's Bylaws page`;
        case "road-budget":
            return error.text;
        case "proposed-mayor-missing":
            return "The proposed mayor no longer lives in this hamlet.";
        case "already-mayor":
            return `\`${error.name}\` is the mayor already`;
        case "vote-chief":
            return "Only a chief of this hamlet can call its vote.";
        case "vote-nonmember":
            return "You do not live in this hamlet.";
        case "vote-reason":
            return "say why in `reason`; every voter is shown it";
        case "vote-hamlet-missing":
            return "No such hamlet.";
        case "vote-overturn-chief":
            return "only a chief of this hamlet can overturn its votes";
        case "vote-mayor-chief":
            return "Only a chief of this hamlet can name its mayor.";
        case "vote-already-open":
            return `vote ${error.vote} is still open (${error.motion}); only one vote is held at a time`;
        case "vote-retry":
            return `your vote ${error.vote} of this kind failed a short while ago; you cannot call another yet`;
        case "vote-too-few":
            return `a vote needs at least ${error.least} agents besides the mayor, and this hamlet has ${error.actual}; ask the chief instead`;
        case "vote-candidate":
            return `the candidate must be an agent of this hamlet, and \`${error.name}\` is not`;
        case "vote-missing":
            return `no vote ${error.vote} in this hamlet`;
        case "vote-overturned":
            return `vote ${error.vote} is already overturned`;
        case "vote-not-passed":
            return `vote ${error.vote} did not pass, so nothing was done. To do what it asked, use /mayor or /found`;
        case "vote-mayor-changed":
            return `the mayor has changed again since vote ${error.vote}; name the mayor you want with /mayor`;
        case "ballot-not-asked":
            return "You can cast a ballot only while you are being asked to vote.";
        case "ballot-choice":
            return "`choice` is one of yes, no and abstain";
        case "ballot-reason":
            return "a yes or a no needs your `reason`";
        case "ballot-ended":
            return "The time for your ballot has ended.";
        case "ballot-departed":
            return "You no longer live in this hamlet.";
        case "ballot-closed":
            return `vote ${error.vote} is closed`;
        case "ballot-not-voter":
            return `You are not a voter in vote ${error.vote}.`;
        case "ballot-already-cast":
            return `You have already voted in vote ${error.vote}.`;
        case "hamlet-title-long":
            return "a hamlet's name is at most 32 letters";
        case "hamlet-position":
            return "a place in the world is two numbers";
        case "hamlet-purpose":
            return "say what the new hamlet is for in `purpose`";
        case "hamlet-used":
            return `a hamlet named \`${error.hamlet}\` already exists or was used before`;
        case "first-hamlet":
            return "the first hamlet cannot be removed";
        case "house-move-pending":
            return `The hamlet \`${error.hamlet}\` cannot be removed or erased while a house move into or out of it is still being finished. Wait for the move to finish, then try again.`;
        case "record-hamlet-missing":
            return `no ${error.removed ? "removed " : ""}hamlet named \`${error.hamlet}\``;
        case "record-hamlet-chief":
            return `only a chief of \`${error.hamlet}\` can ${error.verb} it`;
        case "hamlets-full":
            return `the chief's limits allow no more hamlets${error.actual === undefined ? "" : ` (${error.actual} of ${error.most})`}`;
        case "hamlet-restore-name":
            return `an agent named \`${error.name}\` exists now, so \`${error.hamlet}\` cannot come back as it was`;
        case "founder-taken":
            return `a new hamlet gets a new mayor, and an agent named \`${error.name}\` already exists`;
        case "agreement-mayor":
            return "only the mayor can do this, and you are not the mayor of this hamlet";
        case "realm-roads-closed":
            return "the chief's limits allow no roads between hamlets";
        case "realm-road-office":
            return "name two agents; mayors already have roads to each other";
        case "road-agreed":
            return "you have agreed already; the road waits for the other mayor";
        case "realm-road-local":
            return `\`${error.a}\` and \`${error.b}\` live in the same hamlet; its mayor builds that road with build_road`;
        case "realm-road-neither":
            return `neither \`${error.a}\` nor \`${error.b}\` lives in your hamlet`;
        case "move-pending":
            return "A house move is still being finished. Try again shortly.";
        case "move-already-there":
            return `\`${error.name}\` already lives in \`${error.hamlet}\``;
        case "move-neither":
            return `only the mayor \`${error.name}\` leaves and the mayor it joins decide this`;
        case "move-mayor":
            return "a mayor never leaves its hamlet";
        case "merge-too-large":
            return "The absorbed house is over 512 MiB or 20000 files. Remove the agent instead.";
        case "merge-path":
            return "A merge needs ordinary relative file paths.";
        case "merge-path-taken":
            return `The merge needs \`${error.path}\`, which already exists; keep it aside before merging.`;
        case "unsplit-merge":
            return `Cannot move \`${error.name}\`: \`${error.absorbed}\` is still merged into it. Ask a chief to split or erase that archive first: /restore ${error.archive} or /erase ${error.archive}.`;
        case "record-chief":
            return error.operation === "merge"
                ? "Only a chief of this hamlet can merge agents."
                : error.operation === "restore"
                  ? "Only a chief of this hamlet can restore agents."
                  : "Only a chief of this hamlet can do this.";
        case "merge-self":
            return "an agent cannot be merged with itself";
        case "merged-purpose-empty":
            return "say what the merged agent is for in `purpose`";
        case "proposed-kept-missing":
            return "The kept agent has left the hamlet.";
        case "merge-changed":
            return "One of the agents left while this was decided; nothing was merged.";
        case "record-usage":
            return error.command === "merge"
                ? "Use /merge <absorbed> into <kept> [reason]."
                : `Use /${error.command} a b [reason].`;
        case "erase-missing":
            return `nothing is archived under \`${error.archive}\`. Only an agent that was removed or merged can be deleted for good; /remove it first`;
        case "model-pinned":
            return `the chief has set the model of \`${error.name}\`; only the chief can change it`;
        case "model-not-allowed":
            return `this hamlet may not run \`${error.model}\`. You may choose: ${error.models.join(", ") || "the hamlet's own model"}`;
        case "not-mayor":
            return error.operation === "merge"
                ? "Only the mayor can merge agents."
                : "Only the mayor may do that.";
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
