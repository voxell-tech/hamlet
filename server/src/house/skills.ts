import { lstat } from "node:fs/promises";
import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import type { FileErrorCode } from "@earendil-works/pi-durable/env";
import { buildHouse, houseOf, type Layout } from "../home/layout.ts";
import { type AgentRef, agentKey } from "../record.ts";
import { err, ok, type Result } from "../result.ts";
import type { SandboxEnv } from "../sandbox.ts";
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

/** Notes that could not be read or changed safely. */
export type NotesFailure =
    | ({ operation: "read-notes"; agent: AgentRef; retrySafe: true } & (
          | { kind: "notes-missing" }
          | { kind: "notes-not-utf8" }
          | { kind: "notes-unreadable" }
      ))
    | ({ operation: "write-notes"; agent: AgentRef; retrySafe: false } & (
          | { kind: "notes-unavailable" }
          | { kind: "notes-write-refused"; code: FileErrorCode; text: string }
      ));

/** Safe words for a refused notes operation. */
export function notesFailureText(error: NotesFailure): string {
    switch (error.kind) {
        case "notes-missing":
            return "The agent's NOTES.md is missing. Create its notes before trying again.";
        case "notes-not-utf8":
            return "The agent's NOTES.md is not UTF-8 text. Keep its original bytes aside and correct the file.";
        case "notes-unreadable":
            return "The agent's NOTES.md could not be read safely. Inspect its house before trying again.";
        case "notes-unavailable":
            return "The agent's house could not be opened.";
        case "notes-write-refused":
            return error.text;
    }
    const unhandled: never = error;
    throw new Error(`Unclassified notes failure: ${unhandled}`);
}

/** One house's bounded read-only notes and skill papers. */
export class House {
    /** The stable location of this agent's house. */
    readonly path: string;
    readonly #agent: AgentRef;
    readonly #layout: Layout;
    readonly #reader: (agent: AgentRef, context: Context) => SandboxEnv;
    #env: SandboxEnv | undefined;
    #names: Promise<string[]> | undefined;
    #until = 0;

    /** Readers are supplied by the home's confinement seam. */
    constructor(
        layout: Layout,
        agent: AgentRef,
        reader: (agent: AgentRef, context: Context) => SandboxEnv,
    ) {
        this.#layout = layout;
        this.#agent = { ...agent };
        this.path = houseOf(layout, agent);
        this.#reader = reader;
    }

    #environment(context: Context): SandboxEnv {
        this.#env ??= this.#reader(this.#agent, context);
        return this.#env;
    }

    /** Missing notes and invalid UTF-8 are distinct refusals; original bytes are never replaced. */
    async notes(
        context: Context,
    ): Promise<
        Result<
            { text: string; bytes: Buffer },
            Extract<NotesFailure, { operation: "read-notes" }>
        >
    > {
        const read = await this.#environment(context).readBinaryFile(
            join(this.path, "NOTES.md"),
            context,
        );
        if (!read.ok)
            return err({
                kind:
                    read.error.code === "not_found"
                        ? "notes-missing"
                        : "notes-unreadable",
                operation: "read-notes",
                agent: this.#agent,
                retrySafe: true,
            });
        const bytes = Buffer.from(read.value);
        try {
            const text = new TextDecoder("utf-8", {
                fatal: true,
                ignoreBOM: true,
            }).decode(bytes);
            return ok({ text, bytes });
        } catch {
            return err({
                kind: "notes-not-utf8",
                operation: "read-notes",
                agent: this.#agent,
                retrySafe: true,
            });
        }
    }

    /** Notes for prompt excerpts; missing, unsafe or undecodable notes contribute no text. */
    async notesText(context: Context): Promise<string> {
        const read = await this.notes(context);
        if (read.ok) return read.value.text;
        switch (read.error.kind) {
            case "notes-missing":
            case "notes-not-utf8":
            case "notes-unreadable":
                return "";
        }
        const unhandled: never = read.error;
        throw new Error(`Unclassified notes read: ${unhandled}`);
    }

    async #shelf(contents: boolean, context: Context): Promise<Skill[]> {
        if (!(await lstat(this.path).catch(() => undefined))?.isDirectory())
            return [];
        const env = this.#environment(context);
        const dir = join(this.path, "skills");
        const listed = await env.listDir(dir, context);
        if (!listed.ok) return [];
        const found: Skill[] = [];
        for (const entry of listed.value) {
            if (entry.kind !== "directory") continue;
            const path = join(dir, entry.name, "SKILL.md");
            const opened = await env.openBinaryReader(
                path,
                { noFollow: true },
                context,
            );
            if (!opened.ok) continue;
            let text = "";
            try {
                const info = await opened.value.info(context);
                if (!info.ok || info.value.kind !== "file") continue;
                if (contents) {
                    const read = await opened.value.read(0, 64 * 1024, context);
                    if (read.ok)
                        text = Buffer.from(read.value).toString("utf8");
                }
            } finally {
                await opened.value.close(context);
            }
            found.push({
                name: entry.name,
                description: "",
                usage: undefined,
                ...parseSkill(text),
                folder: entry.name,
            });
        }
        return found.sort((a, b) =>
            a.folder < b.folder ? -1 : a.folder > b.folder ? 1 : 0,
        );
    }

    /** Bounded papers read under this house's own policy. */
    skills(context: Context): Promise<Skill[]> {
        this.#until = 0;
        return this.#shelf(true, context);
    }

    /** Names cached briefly until an observed house change invalidates them. */
    skillNames(context: Context): Promise<string[]> {
        if (this.#names === undefined || this.#until <= Date.now()) {
            this.#until = Date.now() + 1000;
            this.#names = this.#shelf(false, context).then((skills) =>
                skills.map((skill) => skill.folder),
            );
        }
        return this.#names;
    }

    /** Creates only missing initial files. */
    build(): Promise<string> {
        return buildHouse(this.#layout, this.#agent);
    }

    /** Invalidates the names without replacing a live helper. */
    invalidate(): void {
        this.#until = 0;
    }

    /** Releases the read-only helper owned by this house. */
    async close(context: Context): Promise<void> {
        await this.#env?.cleanup(context);
        this.#env = undefined;
        this.#names = undefined;
    }
}

/** House readers owned and closed by one open home. */
export class Houses {
    readonly #kept = new Map<string, House>();
    #delivery: Promise<unknown> = Promise.resolve();
    readonly #layout: Layout;
    readonly #reader: (agent: AgentRef, context: Context) => SandboxEnv;

    /** Takes the home's layout and its read-only constructor. */
    constructor(
        layout: Layout,
        reader: (agent: AgentRef, context: Context) => SandboxEnv,
    ) {
        this.#layout = layout;
        this.#reader = reader;
    }

    /** Returns the same house handle for an agent's stable identity. */
    of(agent: AgentRef): House {
        const key = agentKey(agent);
        let house = this.#kept.get(key);
        if (house === undefined) {
            house = new House(this.#layout, agent, this.#reader);
            this.#kept.set(key, house);
        }
        return house;
    }

    /** Invalidates every shelf below a changed root. */
    invalidate(root: string): void {
        for (const house of this.#kept.values())
            if (house.path === root || house.path.startsWith(`${root}/`))
                house.invalidate();
    }

    /** Serializes tree claims across every receiving house. */
    async deliver<T>(work: () => Promise<T>): Promise<T> {
        const next = this.#delivery.catch(() => {}).then(work);
        this.#delivery = next;
        return next;
    }

    /** Closes every reader, including houses no longer in the record. */
    async close(context: Context): Promise<void> {
        await Promise.all(
            [...this.#kept.values()].map((house) => house.close(context)),
        );
        this.#kept.clear();
    }
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
