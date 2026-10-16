import { randomUUID } from "node:crypto";
import { dirname, join } from "node:path";
import type { Context } from "@earendil-works/chord";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import {
    closedPolicy,
    platformSandbox,
    type Sandbox,
    SandboxEnv,
} from "./sandbox.ts";
import { shelf as readShelf } from "./skill-paper.ts";

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

const SHELF_MS = 1000;
const shelves = new Map<
    string,
    {
        env: SandboxEnv;
        until: number;
        names?: Promise<string[]>;
    }
>();

function houseShelf(house: string, sandbox: Sandbox) {
    const dir = join(house, "skills");
    let shelf = shelves.get(dir);
    if (shelf === undefined) {
        const policy = closedPolicy(house, [house]);
        policy.rw = [];
        policy.env = { PATH: "/usr/bin:/bin", HOME: house, LANG: "C.UTF-8" };
        shelf = {
            until: 0,
            env: new SandboxEnv({
                cwd: house,
                policy,
                sandbox,
                tmpScope: `papers-${randomUUID()}`,
            }),
        };
        shelves.set(dir, shelf);
    }
    return shelf;
}

/** Skill folders containing a regular SKILL.md, listed inside the house's sandbox (C8, C18). */
export function skillNames(dir: string): Promise<string[]> {
    const shelf = houseShelf(dirname(dir), platformSandbox());
    if (shelf.names === undefined || shelf.until <= Date.now()) {
        shelf.until = Date.now() + SHELF_MS;
        shelf.names = shelf.env
            .skillShelf(dir, false, BACKGROUND_CONTEXT)
            .then((read) =>
                read.ok ? read.value.map((paper) => paper.folder) : [],
            );
    }
    return shelf.names;
}

/** Invalidated shelves below a changed house or home. */
export function refreshSkills(root: string): void {
    for (const [dir, shelf] of shelves)
        if (dir === root || dir.startsWith(`${root}/`)) shelf.until = 0;
}

/** Closed sandbox readers belonging to a house or home. */
export async function closeSkills(
    root: string,
    context: Context,
): Promise<void> {
    const closing = [...shelves].filter(
        ([dir]) => dir === root || dir.startsWith(`${root}/`),
    );
    for (const [dir] of closing) shelves.delete(dir);
    await Promise.all(closing.map(([, shelf]) => shelf.env.cleanup(context)));
}

/** The house's skill papers listed and read together under its own sandbox policy (C8, C18). */
export async function houseSkills(
    house: string,
    sandbox: Sandbox,
    context: Context,
): Promise<Skill[]> {
    const shelf = houseShelf(house, sandbox);
    shelf.until = 0;
    const read = await shelf.env.skillShelf(
        join(house, "skills"),
        true,
        context,
    );
    return read.ok
        ? read.value.map(({ folder, text }) => ({
              name: folder,
              description: "",
              usage: undefined,
              ...parseSkill(text),
              folder,
          }))
        : [];
}

/** A server-owned skill shelf with bounded, regular-file papers (C8). */
export async function skillsIn(dir: string): Promise<Skill[]> {
    return (await readShelf(dir, true)).map(({ folder, text }) => ({
        name: folder,
        description: "",
        usage: undefined,
        ...parseSkill(text),
        folder,
    }));
}

/** Shipped skill names and descriptions for the chief's gift menu (H25). */
export async function starterSkills(
    dir: string,
): Promise<{ name: string; description: string }[]> {
    return (await skillsIn(dir)).map(({ folder, description }) => ({
        name: folder,
        description: description.split(/\r?\n/)[0] ?? "",
    }));
}
