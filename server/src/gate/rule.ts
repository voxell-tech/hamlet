import type { Leave } from "../environment.ts";
import type { Limits } from "../limits/rules.ts";
import { type Asked, refused } from "../oversight/asked.ts";
import type { Danger } from "../oversight/waits.ts";
import type { Call } from "./call.ts";

/** The permissions and mode in force at a call. */
export type Standing = {
    confined: boolean;
    houseEdit?: boolean;
    net: boolean;
    unconfined: boolean;
    asksEdits: boolean;
    asksCommands: boolean;
};

/** The reach granted to a call, or the approval it must await. */
export type Verdict = { run: Leave } | { ask: Danger } | { block: string };

const NETWORK = [
    "curl",
    "wget",
    "http://",
    "https://",
    "ssh",
    "scp",
    "rsync",
    "ftp",
    "ping",
    "nc",
    "git clone",
    "git fetch",
    "git pull",
    "git push",
    "pip install",
    "pip3 install",
    "npm install",
    "cargo install",
    "brew install",
];

/** The H11 decision, ordered by reach, reserved acts and the turn's refusal history. */
export function rule(
    call: Call,
    standing: Standing,
    asked: Readonly<Asked> & { by: string; about: string },
    limits: Limits,
    mayor: boolean,
): Verdict {
    const verdict =
        mayor && mayorReserved(call, limits)
            ? { ask: "reserved" as const }
            : reachRule(call, standing);
    if ("run" in verdict) return verdict;
    const block = refused(
        asked,
        asked.by,
        verdict.ask,
        asked.about,
        limits.asking.refusals,
    );
    return block === undefined ? verdict : { block };
}

/** A society act reserved by the chief's current mayor limits. */
export function mayorReserved(call: Call, limits: Limits): boolean {
    if (call.kind !== "society") return false;
    switch (call.name) {
        case "create_agent":
            return (
                !limits.mayor.createAgents ||
                (!limits.mayor.buildRoads &&
                    Array.isArray(call.args.roads) &&
                    call.args.roads.length > 0)
            );
        case "build_road":
            return !limits.mayor.buildRoads;
        case "remove_agent":
            return !limits.mayor.removeAgents;
        case "merge_agents":
            return !limits.mayor.mergeAgents;
        default:
            return false;
    }
}

function reachRule(
    reach: Call,
    standing: Standing,
): Exclude<Verdict, { block: string }> {
    if (reach.kind === "read" || reach.kind === "society")
        return { run: { net: false } };
    if (reach.kind === "edit") {
        return standing.asksEdits && !standing.houseEdit
            ? { ask: "edit" }
            : { run: { net: false } };
    }
    const command: Verdict = standing.asksCommands
        ? { ask: "command" }
        : { run: { net: standing.net } };
    if (!standing.confined) {
        if (reach.kind === "shell" && destructive(reach.command))
            return { ask: "destructive" };
        return standing.unconfined ? command : { ask: "unconfined" };
    }
    return !standing.net &&
        (reach.net || (reach.kind === "shell" && networked(reach.command)))
        ? { ask: "network" }
        : command;
}

function networked(command: string): boolean {
    const lower = command.toLowerCase();
    return NETWORK.some((needle) => mentions(lower, needle));
}

const DESTRUCTIVE = [
    "sudo",
    "doas",
    "mkfs",
    "of=/dev/",
    "diskutil",
    "shutdown",
    "reboot",
    "launchctl",
    "crontab",
    "push --force",
    "push -f",
    "| sh",
    "| bash",
    "|sh",
    "|bash",
];
const ROOTS = ["/", "~", "$HOME", ".."];

function destructive(command: string): boolean {
    if (DESTRUCTIVE.some((needle) => mentions(command.toLowerCase(), needle)))
        return true;
    const args = command.trim().split(/\s+/);
    const deletes = args.some(
        (arg, index) =>
            arg === "rm" &&
            args
                .slice(index + 1)
                .some(
                    (flag) =>
                        (flag.startsWith("-") &&
                            !flag.startsWith("--") &&
                            flag.toLowerCase().includes("r")) ||
                        flag === "--recursive",
                ),
    );
    return (
        deletes &&
        args.some((arg) => {
            const path = arg.replace(/^["']+|["']+$/g, "");
            return ROOTS.some(
                (root) =>
                    path === root ||
                    (path.startsWith(root) &&
                        (root === "/" ||
                            path.slice(root.length).startsWith("/"))),
            );
        })
    );
}

function mentions(command: string, needle: string): boolean {
    return /^[a-zA-Z0-9_-]+$/.test(needle)
        ? command.split(/[^a-zA-Z0-9_-]/).includes(needle)
        : command.includes(needle);
}
