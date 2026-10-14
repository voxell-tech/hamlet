import type { Leave } from "./environment.ts";
import type { Danger } from "./waits.ts";

/** The reach of a tool call (H11). */
export type Reach =
    | { kind: "read" }
    | { kind: "other" }
    | { kind: "edit" }
    | { kind: "shell"; command: string; net: boolean }
    | { kind: "custom"; net: boolean };

/** The permissions and mode in force at a call. */
export type Standing = {
    confined: boolean;
    net: boolean;
    unconfined: boolean;
    asksEdits: boolean;
    asksCommands: boolean;
};

export type Verdict = { run: Leave } | { ask: Danger };

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

/** The rule for whether a call runs or waits for a chief (danger.rs:172-223). */
export function judge(reach: Reach, standing: Standing): Verdict {
    if (reach.kind === "read" || reach.kind === "other")
        return { run: { net: false } };
    if (reach.kind === "edit") {
        return standing.asksEdits ? { ask: "edit" } : { run: { net: false } };
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
