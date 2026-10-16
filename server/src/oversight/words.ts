import type { Danger, Wait } from "./waits.ts";

const SAYS: Record<Danger, string> = {
    network: "wants the network for",
    unconfined: "wants to run, with no sandbox,",
    destructive: "wants to run, with no sandbox, the destructive",
    reserved: "wants to do what you reserved:",
    edit: "wants to edit",
    command: "wants to run",
};

/** The first `most` characters of `text` on one line, with dots where cut. */
export function clip(text: string, most: number): string {
    const flat = text.trim().split(/\s+/).join(" ");
    const chars = [...flat];
    return chars.length > most ? `${chars.slice(0, most).join("")}...` : flat;
}

/** The danger's words after the agent's name. */
export function says(danger: Danger | null): string {
    return danger === null ? "" : SAYS[danger];
}

/** One line for a wait, quoting at most `most` characters of its subject (queue.rs:60-89). */
export function line(
    wait: Pick<Wait, "kind" | "danger" | "by" | "about">,
    most: number,
): string {
    const { kind, danger, by, about } = wait;
    switch (kind) {
        case "approval":
            return `${by} ${says(danger)} \`${clip(danger === "edit" ? (about.split(/\r?\n/)[0] ?? about) : about, most)}\``;
        case "objection":
            return clip(
                about.split(/\r?\n/)[1] ?? about,
                most + Buffer.byteLength(by),
            );
        case "question":
            return `${by} asks: ${clip(about, most)}`;
        case "fault":
            return `${by} failed: ${clip(about, most)}`;
    }
}

/** A wait in full, with its subject and offered answers (typed.rs:22-36). */
export function strip(
    wait: Readonly<Pick<Wait, "kind" | "danger" | "by" | "about" | "answers">>,
): string {
    return `[${wait.kind}] ${line(wait, 60)}\n    ${wait.about.replaceAll("\n", "\n    ")}\n    answer with: ${wait.answers.map((choice) => `/${choice}`).join(" ")}`;
}

/** The elapsed milliseconds in Rust's words (queue.rs:230-237). */
export function waited(ms: number): string {
    const minutes = Math.floor(ms / 60_000);
    if (minutes < 1) return "under a minute";
    if (minutes < 60) return `${minutes} min`;
    return `${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, "0")} min`;
}
