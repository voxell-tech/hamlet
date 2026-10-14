import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import type { ExecutionEnv } from "@earendil-works/pi-durable/env";

/** A skill as the front matter of its `SKILL.md` tells of it (C8). */
export type Skill = {
    name: string;
    description: string;
    /** The command that runs it. */
    usage: string | undefined;
    /** The name of its folder. */
    folder: string;
};

/**
 * The skill a `SKILL.md` of `text` tells of, `undefined` unless it opens with a front matter that names it (skill.rs:16-46).
 * The front matter runs to the first line opening with `---`, or to the end when none does.
 */
export function parseSkill(text: string): Omit<Skill, "folder"> | undefined {
    if (!text.startsWith("---")) return undefined;
    const front = (text.slice(3).split("\n---")[0] ?? "").split("\n");
    const field = (key: string) => {
        for (const line of front) {
            const at = line.indexOf(":");
            if (at >= 0 && line.slice(0, at).trim() === key) {
                return line
                    .slice(at + 1)
                    .trim()
                    .replace(/^"+|"+$/g, "");
            }
        }
        return undefined;
    };
    const name = field("name");
    if (name === undefined) return undefined;
    return {
        name,
        description: field("description") ?? "",
        usage: field("usage") || undefined,
    };
}

/** The skills in `dir`, one folder each, read through `env`, by name. A folder that cannot be read holds none. */
export async function skillsIn(
    env: ExecutionEnv | undefined,
    dir: string,
    context: Context,
): Promise<Skill[]> {
    const listed = await env?.listDir(dir, context);
    const skills: Skill[] = [];
    for (const entry of listed?.ok ? listed.value : []) {
        if (entry.kind === "file") continue;
        const text = await env?.readTextFile(
            join(dir, entry.name, "SKILL.md"),
            context,
        );
        const skill = text?.ok ? parseSkill(text.value) : undefined;
        if (skill !== undefined) skills.push({ ...skill, folder: entry.name });
    }
    return skills.sort((a, b) =>
        a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
    );
}
