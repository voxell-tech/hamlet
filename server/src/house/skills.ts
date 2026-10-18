import { lstat } from "node:fs/promises";
import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import type {
    ExecutionEnv,
    FileError,
    FileErrorCode,
} from "@earendil-works/pi-durable/env";
import { buildHouse, houseOf, type Layout } from "../home/layout.ts";
import { type AgentRef, agentKey } from "../record.ts";
import { err, ok, type Result } from "../result.ts";
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

/** Opens a read-only environment over one agent's house; its owner closes it. */
export type HouseReader = (agent: AgentRef, context: Context) => ExecutionEnv;

/**
 * Houses kept in a remote workspace instead of the home's folders. Their files are authoritative there; no local copy
 * is made or consulted.
 */
export type RemoteHouses = {
    /** The house's path as the workspace's own environments name it. */
    readonly path: (agent: AgentRef) => string;
    /** Creates missing initial files in the agent's house without replacing existing ones. */
    readonly build: (
        agent: AgentRef,
        context: Context,
    ) => Promise<Result<void, FileError>>;
};

/** One house's bounded read-only notes and skill papers. */
export class House {
    /** The stable location of this agent's house, as its environments name it. */
    readonly path: string;
    readonly #agent: AgentRef;
    readonly #layout: Layout;
    readonly #reader: HouseReader;
    readonly #remote: RemoteHouses | undefined;
    #env: ExecutionEnv | undefined;
    #names: Promise<string[]> | undefined;
    #until = 0;

    /** Readers are supplied by the home's confinement seam; `remote` places the house in a workspace. */
    constructor(
        layout: Layout,
        agent: AgentRef,
        reader: HouseReader,
        remote?: RemoteHouses,
    ) {
        this.#layout = layout;
        this.#agent = { ...agent };
        this.path =
            remote === undefined ? houseOf(layout, agent) : remote.path(agent);
        this.#reader = reader;
        this.#remote = remote;
    }

    #environment(context: Context): ExecutionEnv {
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
        if (this.#env === undefined && this.#remote === undefined) {
            // A newly published agent may not have its house yet. Do not pin
            // a missing root: its creation would look like a replacement.
            try {
                await lstat(this.path);
            } catch (error) {
                if (
                    error instanceof Error &&
                    "code" in error &&
                    error.code === "ENOENT"
                )
                    return [];
                throw error;
            }
        }
        const env = this.#environment(context);
        // The house itself must be a real directory where its environment sees it, not a link.
        const root = await env.fileInfo(this.path, context);
        if (!root.ok && root.error.code === "not_found") {
            // A house not built yet gets a fresh reader once it exists.
            if (this.#env === env) this.#env = undefined;
            await env.cleanup(context);
            return [];
        }
        if (!root.ok || root.value.kind !== "directory") return [];
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

    /**
     * Creates only missing initial files, wherever the house is kept, and returns `path`; existing notes and skills
     * stay as they are. A refused remote house is a returned failure; a local folder failure still throws.
     */
    async build(context?: Context): Promise<Result<string, FileError>> {
        if (this.#remote === undefined)
            return ok(await buildHouse(this.#layout, this.#agent));
        if (context === undefined)
            throw new Error(
                "A remote house build requires its operation context.",
            );
        const built = await this.#remote.build(this.#agent, context);
        return built.ok ? ok(this.path) : built;
    }

    /** Invalidates the names without replacing a live helper. */
    invalidate(): void {
        this.#until = 0;
    }

    /** Releases the read-only environment owned by this house. */
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
    readonly #reader: HouseReader;
    readonly #remote: RemoteHouses | undefined;

    /** Takes the home's layout, its read-only constructor and, for workspace houses, how to reach them. */
    constructor(layout: Layout, reader: HouseReader, remote?: RemoteHouses) {
        this.#layout = layout;
        this.#reader = reader;
        this.#remote = remote;
    }

    /** Returns the same house handle for an agent's stable identity. */
    of(agent: AgentRef): House {
        const key = agentKey(agent);
        let house = this.#kept.get(key);
        if (house === undefined) {
            house = new House(this.#layout, agent, this.#reader, this.#remote);
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
