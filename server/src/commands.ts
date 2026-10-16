import type { Permission } from "./gate.ts";
import type { Choice } from "./oversight/waits.ts";

const TABLE = [
    ["new", "", "Start a new session and keep this one in history"],
    [
        "resume",
        "<number>",
        "Go back to a past session, by its number in History",
    ],
    [
        "retry",
        "",
        "Carry on a turn that failed or was stopped, from where it ended",
    ],
    ["agent", "<name>", "Talk to that agent, in whichever hamlet"],
    ["hamlet", "<name>", "Talk to that hamlet's mayor"],
    ["to", "<agent> <instruction>", "Instruct an agent directly"],
    ["stop", "[agent|all]", "Stop an agent's work, or everyone's"],
    ["queue", "", "List what waits for you"],
    ["allow", "[reason]", "Allow what waits, this once"],
    ["always", "[reason]", "Allow it, and the like from now on"],
    ["deny", "[reason]", "Refuse what waits"],
    ["go", "[reason]", "Tell an agent that objected to go ahead"],
    ["withdraw", "[reason]", "Take back what an agent objected to"],
    ["reply", "<text>", "Answer an agent's question"],
    ["dismiss", "", "Put away a fault or a lapsed request"],
    ["undo", "", "Reverse the latest thing that can be reversed"],
    ["grants", "", "List what agents are allowed for good"],
    [
        "revoke",
        "<agent> <network|unconfined>",
        "Take back a lasting permission",
    ],
    ["remove", "<agent> [reason]", "Remove an agent and archive its house"],
    [
        "merge",
        "<absorbed> into <kept> [reason]",
        "Merge one agent into another",
    ],
    ["road", "<agent> <agent> [reason]", "Build a road between two agents"],
    [
        "unroad",
        "<agent> <agent> [reason]",
        "Take up the road between two agents",
    ],
    [
        "give",
        "<agent> <skill> [from <agent>]",
        "Give an agent a starter skill, or another's",
    ],
    [
        "folder",
        "<folder|none>",
        "Point this hamlet at a folder to work on, or at none",
    ],
    [
        "mode",
        "<manual|auto>",
        "Whether agents ask before they change the folder",
    ],
    ["reset-budget", "", "Start this hamlet's budget again from nothing spent"],
    [
        "restore",
        "<agent> [as <new-name>] [reason]",
        "Bring back a removed or merged agent",
    ],
    [
        "erase",
        "<agent>",
        "Delete a removed agent for good, to free its room on disk",
    ],
    ["record", "", "Who was created, removed, merged and restored"],
    ["budget", "", "The hamlet's budget and each agent's spend"],
    ["reflect", "", "Have the agent reflect on this conversation now"],
    ["mayor", "<agent> [reason]", "Make an agent of this hamlet its mayor"],
    [
        "vote",
        "<agent> [reason]",
        "Ask this hamlet's agents to vote on making that agent their mayor",
    ],
    ["votes", "", "This hamlet's votes, with every ballot"],
    [
        "overturn",
        "<vote> [reason]",
        "Reverse a vote that passed, or stop an open one",
    ],
    ["hamlets", "", "List the hamlets and their mayors"],
    [
        "found",
        "<hamlet> <mayor> <purpose>",
        "Found a hamlet of your own under a new mayor",
    ],
    [
        "remove-hamlet",
        "<hamlet> [reason]",
        "Remove a hamlet and archive its houses",
    ],
    ["restore-hamlet", "<hamlet>", "Bring a removed hamlet back"],
    ["unshelve", "<skill> [reason]", "Take a skill out of the library"],
    ["erase-hamlet", "<hamlet>", "Delete a removed hamlet for good"],
    ["name-hamlet", "<hamlet> <new-name>", "Call a hamlet something else"],
    [
        "move-hamlet",
        "<hamlet> <x> <z>",
        "Set a hamlet's town down somewhere else",
    ],
    ["chief", "", "The commands about hamlets and votes"],
] as const;

/** The chief's typed commands, with their usage and descriptions (H26). */
export const COMMANDS = TABLE.map(([deed, args, description]) => ({
    name: `/${deed}`,
    deed,
    args,
    description,
}));

/** The chief's command names carried out by the server (H26). */
export const DEEDS = TABLE.map(([deed]) => deed);

/** A parsed typed command; unrecognized lines remain words for the agent. */
export type Command =
    | { command: "deed"; deed: string; rest: string }
    | { command: "to"; agent: string; text: string }
    | {
          command:
              | "new"
              | "retry"
              | "reflect"
              | "budget"
              | "reset-budget"
              | "queue"
              | "grants";
      }
    | { command: "resume"; number: number | null }
    | { command: "stop"; agent: string | null }
    | { command: "answer"; choice: Choice; text: string }
    | { command: "revoke"; agent: string; permission: Permission }
    | { command: "folder"; path: string }
    | { command: "mode"; mode: "manual" | "auto" }
    | { command: "give"; agent: string; skill: string; from?: string };

/** A command that needs corrected arguments before it can run. */
export type CommandFailure =
    | { kind: "usage"; command: string; args: string }
    | { kind: "needs-number" };

/** Chief-facing correction for a parsed command's arguments. */
export function commandFailureText(failure: CommandFailure): string {
    switch (failure.kind) {
        case "usage":
            return failure.command === "/reply"
                ? "say what to reply: /reply <text>"
                : `usage: ${failure.command} ${failure.args}`.trim();
        case "needs-number":
            return "Say which: `/resume <number>`, with the number History shows for a past session.";
    }
    const unhandled: never = failure;
    throw new Error(`Unclassified command failure: ${unhandled}`);
}

/** A chief's command, a classified argument failure, or words to the agent (H24). */
export function readCommand(
    text: string,
): Command | CommandFailure | undefined {
    const match = /^(\S+)(?:\s+([\s\S]*))?$/.exec(text.trim());
    const definition = COMMANDS.find((each) => each.name === match?.[1]);
    if (definition === undefined) return undefined;
    const rest = (match?.[2] ?? "").trim();
    const usage = (): CommandFailure => ({
        kind: "usage",
        command: definition.name,
        args: definition.args,
    });
    switch (definition.deed) {
        case "to": {
            const to = /^(\S+)\s+([\s\S]+)$/.exec(rest);
            return to?.[1] && to[2]
                ? { command: "to", agent: to[1], text: to[2] }
                : usage();
        }
        case "new":
        case "retry":
        case "reflect":
        case "budget":
            return rest === "" ? { command: definition.deed } : undefined;
        case "queue":
        case "grants":
        case "reset-budget":
            return { command: definition.deed };
        case "resume":
            if (rest === "") return { command: "resume", number: null };
            if (!/^\d+$/.test(rest) || !Number.isSafeInteger(Number(rest)))
                return { kind: "needs-number" };
            return { command: "resume", number: Number(rest) };
        case "stop":
            return {
                command: "stop",
                agent: rest === "" || rest === "all" ? null : rest,
            };
        case "allow":
        case "always":
        case "deny":
        case "go":
        case "withdraw":
        case "dismiss":
        case "reply":
            if (definition.deed === "reply" && rest === "") return usage();
            return { command: "answer", choice: definition.deed, text: rest };
        case "revoke": {
            const [agent, permission, extra] = rest.split(/\s+/);
            if (
                !agent ||
                extra !== undefined ||
                (permission !== "network" && permission !== "unconfined")
            )
                return usage();
            return { command: "revoke", agent, permission };
        }
        case "folder":
            return { command: "folder", path: rest };
        case "mode":
            return rest === "manual" || rest === "auto"
                ? { command: "mode", mode: rest }
                : usage();
        case "give": {
            const [agent, skill, word, from, extra] = rest.split(/\s+/);
            if (
                !agent ||
                !skill ||
                extra !== undefined ||
                (word !== undefined && (word !== "from" || !from))
            )
                return usage();
            return {
                command: "give",
                agent,
                skill,
                ...(from === undefined ? {} : { from }),
            };
        }
        default:
            return { command: "deed", deed: definition.deed, rest };
    }
}
