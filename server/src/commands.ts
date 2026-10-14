import type { Permission } from "./grants.ts";
import type { Choice } from "./waits.ts";

const TABLE = [
    ["new", "", "Start a new session and keep this one in history"],
    [
        "resume",
        "[number]",
        "List past sessions, or go back to one by its number",
    ],
    ["retry", "", "Carry on a turn that failed or was stopped"],
    ["reflect", "", "Have the agent reflect on this conversation now"],
    ["stop", "[agent|all]", "Stop an agent's work, or everyone's"],
    ["queue", "", "List what waits for you"],
    ["allow", "[reason]", "Allow what waits, this once"],
    ["always", "[reason]", "Allow it, and the like from now on"],
    ["deny", "[reason]", "Refuse what waits"],
    ["go", "[reason]", "Tell an agent that objected to go ahead"],
    ["withdraw", "[reason]", "Take back what an agent objected to"],
    ["reply", "<text>", "Answer an agent's question"],
    ["dismiss", "[reason]", "Put away a fault"],
    ["grants", "", "List what agents are allowed for good"],
    [
        "revoke",
        "<agent> <network|unconfined>",
        "Take back a lasting permission",
    ],
    ["budget", "", "Show the hamlet's spend"],
    ["reset-budget", "", "Start the budget's period again"],
    [
        "folder",
        "[folder|none]",
        "Show or change the folder the hamlet works on",
    ],
    ["mode", "<manual|auto>", "Change whether edits and commands wait"],
    ["give", "<agent> <starter>", "Give an agent a starter skill"],
    ["undo", "", "Reverse the latest thing that can be reversed"],
] as const;

/** The chief's typed commands, with their usage and descriptions (H26). */
export const COMMANDS = TABLE.map(([deed, args, description]) => ({
    name: `/${deed}`,
    deed,
    args,
    description,
}));

export type Command =
    | {
          command:
              | "new"
              | "retry"
              | "reflect"
              | "budget"
              | "reset-budget"
              | "queue"
              | "grants"
              | "undo";
      }
    | { command: "resume"; number: number | null }
    | { command: "stop"; agent: string | null }
    | { command: "answer"; choice: Choice; text: string }
    | { command: "revoke"; agent: string; permission: Permission }
    | { command: "folder"; path: string }
    | { command: "mode"; mode: "manual" | "auto" }
    | { command: "give"; agent: string; skill: string };

/** A chief's command, a usage error, or words to the agent (H24). */
export function readCommand(
    text: string,
): Command | { error: string; kind?: "error" } | undefined {
    const match = /^(\S+)(?:\s+([\s\S]*))?$/.exec(text.trim());
    const definition = COMMANDS.find((each) => each.name === match?.[1]);
    if (definition === undefined) return undefined;
    const rest = (match?.[2] ?? "").trim();
    const usage = (kind?: "error") => ({
        error: `usage: ${definition.name} ${definition.args}`.trim(),
        ...(kind === undefined ? {} : { kind }),
    });
    switch (definition.deed) {
        case "new":
        case "retry":
        case "reflect":
        case "budget":
            return rest === "" ? { command: definition.deed } : undefined;
        case "queue":
        case "grants":
        case "undo":
        case "reset-budget":
            return { command: definition.deed };
        case "resume":
            if (rest === "") return { command: "resume", number: null };
            if (!/^\d+$/.test(rest) || !Number.isSafeInteger(Number(rest)))
                return {
                    error: "Say which: `/resume <number>`, with the number History shows for a past session.",
                };
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
            if (definition.deed === "reply" && rest === "")
                return { error: "say what to reply: /reply <text>" };
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
                : usage("error");
        case "give": {
            const [agent, skill, extra] = rest.split(/\s+/);
            return agent && skill && extra === undefined
                ? { command: "give", agent, skill }
                : usage("error");
        }
    }
}
