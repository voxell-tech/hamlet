import { defineDocFamily } from "@earendil-works/pi-durable";

/** The id of the first hamlet. */
export const HOME = "home";

/** The address of the mayor's office: it reaches whoever holds it (H27). */
export const MAYOR = "mayor";

/** The name of the first mayor. */
export const FOUNDER = "founder";

/** An agent, by its hamlet and its number in the hamlet's record (C9). */
export type AgentRef = {
    hamlet: string;
    number: number;
};

/** The key of an agent's member in a family of documents kept per agent. */
export function agentKey(agent: AgentRef): string {
    return `${agent.hamlet}:${agent.number}`;
}

/** The folder a hamlet works on, those it worked on before, and whether its agents ask before changing it (C20, C21). */
export type Work = {
    folder: string | null;
    mode: "manual" | "auto";
    before: string[];
};

/** Each hamlet's work, by hamlet id. */
export const WorkDoc = defineDocFamily<Work, string>({
    kind: "hamlet.work",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ folder: null, mode: "manual", before: [] }),
});

/** An agent name normalized from a model's spelling (C10). */
export function tidyName(name: string): string {
    let tidy = "";
    for (const c of name.trim()) {
        if (/[A-Z]/.test(c) && /[a-z]$/.test(tidy)) tidy += "-";
        if (c === " " || c === "_" || c === "-") {
            if (!tidy.endsWith("-")) tidy += "-";
        } else tidy += c.replace(/[A-Z]/g, (c) => c.toLowerCase());
    }
    return tidy.replace(/^-+|-+$/g, "");
}

/** Validation of an agent's name (C10). */
export function checkName(name: string): void {
    if (!/^[a-z-]{1,24}$/.test(name))
        throw new Error(
            "a name is 1 to 24 lowercase letters and hyphens, such as `librarian`",
        );
    if ([MAYOR, FOUNDER, "all"].includes(name))
        throw new Error(`\`${name}\` is reserved`);
}
