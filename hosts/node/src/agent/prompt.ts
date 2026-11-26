/**
 * The system prompt, one pi-durable section per part (C11), rendered before
 * every model request, so a change shows from the next call. The words are
 * today's (`crates/hamlet_server/src/talk/prompt.rs`), "lodge" and
 * "scholar" included.
 */
import {
    BACKGROUND_CONTEXT,
    type Context,
} from "@earendil-works/chord/context";
import {
    type ConversationId,
    defineExtension,
    type Extension,
    type PromptInput,
    section,
} from "@earendil-works/pi-durable";
import type { AgentContext } from "./actor.ts";
import type { ReadCopies } from "./copies.ts";
import type { AgentFilesystem, FileEntry } from "./filesystem.ts";
import type { AgentInput, Place, PromptSection } from "./messages/agent.ts";
import type { PlaceText, ReadPlace } from "./messages/realm.ts";
import type { ReadAnswer, ToolList } from "./messages/workspace.ts";
import { type Peers, readFile, reasonOf } from "./turn.ts";

/** Characters of the memory's root files together, and of the project's instructions, a prompt shows. */
const MEMORY_SHOWN = 8000;
const CHARACTER_SHOWN = 4000;
/** The files that hold a project's own instructions for agents, the first that reads winning. */
const INSTRUCTIONS = ["AGENTS.md", "CLAUDE.md"] as const;

/** The agent's kind and place as its row holds them now. */
export interface Where {
    readonly kind: AgentInput["kind"];
    readonly place: Omit<Place, "version">;
}

/** What the prompt's parts are read from. */
export interface PromptSources {
    readonly copies: ReadCopies;
    readonly files: AgentFilesystem;
    readonly where: () => Promise<Where>;
    /** A file of the project, by its absolute path, through the workspace; `undefined` when the workspace failed. */
    readonly readFile: (path: string) => Promise<ReadAnswer | undefined>;
    readonly tools: () => Promise<ToolList>;
    readonly readPlace: Placing["read"];
    readonly report: Peers["report"];
    /** The library's checkout, by its absolute path. */
    readonly library?: string;
    /** An errand's prompt is its own instructions only. */
    readonly errand?: string;
}

export function promptSources(
    c: AgentContext,
    copies: ReadCopies,
    peers: Peers,
    placing: Placing,
    tools: () => Promise<ToolList>,
): PromptSources {
    return {
        copies,
        files: peers.files,
        where: async () => {
            const row = await c.db.get<{ kind: Where["kind"]; place: string }>(
                "SELECT kind, place FROM agent WHERE id = 1",
            );
            if (row === undefined)
                throw new Error("the agent has no row: it was not created");
            return {
                kind: row.kind,
                place: JSON.parse(row.place) as Where["place"],
            };
        },
        readFile: (path) =>
            readFile(peers, { path }).catch((error: unknown) => {
                peers.report(`${path} was not read for the prompt`, error);
                return undefined;
            }),
        tools,
        readPlace: placing.read,
        report: peers.report,
    };
}

/**
 * The prompt's sections the realm writes, read once for each request. A
 * request whose read failed is not sent: its sections would show an older
 * place.
 */
export interface Placing {
    /** The sections of the request `input` prepares, or of the chief's chat. */
    read(input: PromptInput | undefined): Promise<PlaceText>;
    /** Why the last request of `conversation` has no place, when it has none. */
    missing(conversation: ConversationId): string | undefined;
}

export function placingOf(c: AgentContext, peers: Peers): Placing {
    const asked = new WeakMap<PromptInput, Promise<PlaceText>>();
    const failed = new Map<ConversationId, string>();
    const readFor = async (
        conversation: ConversationId | undefined,
    ): Promise<PlaceText> => {
        const message =
            conversation === undefined
                ? CHIEFS_CHAT
                : await readPlaceOf(c, conversation);
        try {
            const text = await peers.reach.read_place(message);
            if (conversation !== undefined) failed.delete(conversation);
            return text;
        } catch (error) {
            if (conversation !== undefined) {
                failed.set(conversation, reasonOf(error));
            }
            throw error;
        }
    };
    return {
        read: (input) => {
            if (input === undefined) return readFor(undefined);
            const known = asked.get(input);
            if (known !== undefined) return known;
            const reading = readFor(input.conversationId);
            asked.set(input, reading);
            return reading;
        },
        missing: (conversation) => failed.get(conversation),
    };
}

const CHIEFS_CHAT: ReadPlace = { conversation: "chat" };

/** The kind `conversation` is to the realm: a chat of the agent's, else another. */
async function readPlaceOf(
    c: AgentContext,
    conversation: ConversationId,
): Promise<ReadPlace> {
    const row = await c.db.get<{
        kind: "chief" | "letters" | "questions";
        peer: number;
    }>("SELECT kind, peer FROM chat WHERE conversation = ?", conversation);
    if (row === undefined) return { conversation: "other" };
    if (row.kind === "chief") return CHIEFS_CHAT;
    return { conversation: row.kind, peer: row.peer };
}

type Render = (
    sources: PromptSources,
    context: Context,
    place: () => Promise<PlaceText>,
) => Promise<string | undefined>;

function partsOf(
    sources: PromptSources,
): readonly (readonly [string, Render])[] {
    const { errand } = sources;
    if (errand !== undefined) return [["errand", async () => errand]];
    return [
        ["charter", charter],
        ["machine", machine],
        ["project", project],
        ["character", keptMemory(character)],
        ["memory", keptMemory(memory)],
        ["skills", keptMemory(skills)],
        ["place", placed("place")],
        ["realm", placed("realm")],
        ["scholars", placed("profiles")],
    ];
}

/** A section the realm writes. */
function placed(key: keyof PlaceText): Render {
    return async (_sources, _context, place) => (await place())[key];
}

function keptMemory(render: Render): Render {
    return async (sources, context, place) =>
        (await sources.where()).kind === "worker"
            ? undefined
            : render(sources, context, place);
}

async function rendered(
    render: Render,
    sources: PromptSources,
    context: Context,
    place: () => Promise<PlaceText>,
): Promise<string | undefined> {
    const text = await render(sources, context, place);
    return text?.trim() === "" ? undefined : text;
}

export function promptPart(sources: PromptSources): Extension {
    return defineExtension({
        name: "hamlet.prompt",
        sections: partsOf(sources).map(([key, render]) =>
            section(
                key,
                (input, context) =>
                    rendered(render, sources, context, () =>
                        sources.readPlace(input),
                    ),
                { tag: false },
            ),
        ),
    });
}

export async function promptSections(
    sources: PromptSources,
): Promise<PromptSection[]> {
    let reading: Promise<PlaceText> | undefined;
    const place = () => {
        reading ??= sources.readPlace(undefined);
        return reading;
    };
    const sections = await Promise.all(
        partsOf(sources).map(async ([name, render]) => {
            const text = await rendered(
                render,
                sources,
                BACKGROUND_CONTEXT,
                place,
            );
            return text === undefined ? [] : [{ name, text }];
        }),
    );
    return sections.flat();
}

async function charter(sources: PromptSources): Promise<string | undefined> {
    return (await sources.copies()).charter;
}

const SYSTEM =
    process.platform === "darwin"
        ? "System: macOS, whose shell tools are the BSD ones. Flags that only GNU has, such as `grep -P`, `cat -A` and `sed -r`, fail."
        : process.platform === "win32"
          ? "System: Windows. `powershell` runs PowerShell. `bash`, where it is offered, is Git Bash, which writes C:\\Users as /c/Users."
          : "System: Linux.";

/** As `machine` (`crates/hamlet_server/src/talk/prompt.rs`). */
async function machine(sources: PromptSources): Promise<string> {
    const { settings, model } = await sources.copies();
    const { confinement, faults } = await sources.tools();
    const sandbox =
        confinement.type === "sandbox"
            ? confinement.name
            : confinement.type === "insecure"
              ? "none. Commands are not confined: they reach whatever the chief's account can"
              : "the remote workspace's";
    const net =
        settings?.allow_net === true
            ? "on"
            : "off unless a chief allows it; a command that needs it waits until the chief answers, so set `network` to true on it and carry on";
    const named = model === undefined ? "" : ` Model: ${model.model}.`;
    const failed =
        faults.length === 0
            ? ""
            : `\nTools that failed to load:\n${faults.map(({ folder, words }) => `${folder}: ${words}`).join("\n")}`;
    return `# Environment\nSandbox: ${sandbox}. Network for tools: ${net}.${named}${failed}\n${SYSTEM}\nYour lodge: ${sources.files.root}. It is yours: your memory/ and skills/ live there, and what you write there is kept for your scholar.`;
}

async function project(sources: PromptSources): Promise<string | undefined> {
    const { folder, manual } = (await sources.where()).place;
    if (folder === undefined) return undefined;
    const root = sources.files.root;
    const lines = [
        `Your hamlet works on the folder ${folder}. The work a chief asks for is done there:`,
        `- A command runs with ${folder} as its working directory. It may write there and in your lodge, and nowhere else. It can read the rest of the machine, so the project's own tools run as they do for the chief.`,
        "- `read`, `write` and `edit` take a path relative to the project, or a full path. `read`, `search` and `find` look at files without running a command; use them rather than cat, grep and find.",
        `- Your lodge is ${root}. Keep your memory and skills there, never in the project, and give \`write\` and \`edit\` their full path.`,
    ];
    if (await isRepository(sources, folder)) {
        lines.push(
            "- The project is a git repository. When two agents must work apart, one may use a git worktree inside the project folder, such as under `.hamlet/worktrees/<name>`.",
        );
    }
    if (manual) {
        lines.push(
            "- A chief approves each edit and each command in the project before it runs, so each one waits. `read`, `search` and `find` never wait: learn what you need with them, then make few, well-aimed edits and commands.",
        );
    }
    for (const name of INSTRUCTIONS) {
        const answer = await sources.readFile(`${folder}/${name}`);
        if (answer?.type !== "text") continue;
        const text = cut(
            answer.text,
            MEMORY_SHOWN,
            `[${name} is cut here.]`,
            answer.truncation !== undefined,
        );
        lines.push(`\n# The project's ${name}\n${text}`);
        break;
    }
    return `# The project\n${lines.join("\n")}`;
}

/** Whether `folder` has a `.git`: a repository's folder, or a worktree's file. */
async function isRepository(
    sources: PromptSources,
    folder: string,
): Promise<boolean> {
    for (const path of [`${folder}/.git/HEAD`, `${folder}/.git`]) {
        if ((await sources.readFile(path))?.type === "text") return true;
    }
    return false;
}

async function character(sources: PromptSources): Promise<string | undefined> {
    const text = (await sources.copies()).character?.trim();
    if (text === undefined || text === "") return undefined;
    return `# Your character\n${cut(text, CHARACTER_SHOWN, "[The character is cut here.]")}`;
}

async function memory(
    sources: PromptSources,
    context: Context,
): Promise<string | undefined> {
    const { files } = sources;
    const entries = await listed(sources, "memory", context);
    if (entries === undefined) return undefined;
    let shown = "";
    const folders: string[] = [];
    for (const entry of entries) {
        if (entry.kind === "file") {
            const body = await readText(files, `memory/${entry.name}`, context);
            if (body !== undefined) {
                shown += `\n## memory/${entry.name}\n${body.trim()}\n`;
            }
        } else if (entry.kind === "directory") {
            const index = await readText(
                files,
                `memory/${entry.name}/MEMORY.md`,
                context,
            );
            if (index !== undefined) {
                folders.push(
                    `- memory/${entry.name}/: ${field(index, "description:")}\n`,
                );
            }
        }
    }
    let text =
        "# Your memory\nWhat you keep in memory/ is shown here before each call: its files, then each folder's description, whose `MEMORY.md` you read when you need it. Keep it short and true.\n";
    if (shown === "" && folders.length === 0) return `${text}(empty)`;
    text += cut(
        shown,
        MEMORY_SHOWN,
        "[The memory is longer than is shown and is cut here. Shorten it so all of it is shown.]",
    );
    if (folders.length > 0) text += `\nFolders:\n${folders.join("")}`;
    return text.trimEnd();
}

/**
 * As today's `skills` (`crates/hamlet_server/src/talk/prompt.rs:411-458`):
 * the agent's own skills, then the library's shelves, when they hold any.
 */
async function skills(
    sources: PromptSources,
    context: Context,
): Promise<string> {
    const { files } = sources;
    // As today's `shelf`, a folder that cannot be listed shows no skills.
    const entries = (await listed(sources, "skills", context)) ?? [];
    const quoted = files.root.replaceAll("'", "'\\''");
    let text = "# Your skills\n";
    let own = 0;
    for (const entry of entries) {
        if (entry.kind !== "directory") continue;
        const paper = await readText(
            files,
            `skills/${entry.name}/SKILL.md`,
            context,
        );
        if (paper === undefined) continue;
        const name = field(paper, "name:");
        const usage = field(paper, "usage:");
        const run = usage === "" ? "" : ` Run: \`cd '${quoted}' && ${usage}\``;
        text += `- ${name === "" ? entry.name : name}: ${field(paper, "description:")}${run} (skills/${entry.name}/SKILL.md)\n`;
        own += 1;
    }
    if (own === 0) text += "(none yet)";
    const { library } = sources;
    const shared = (await sources.copies()).shelves ?? [];
    if (library !== undefined && shared.length > 0) {
        text += `\n# The library\nSkills every agent shares, kept in ${library}. Read one and run it from there; do not copy it into your lodge. You cannot write there. To share a skill of your own, use \`submit_skill\`.\n`;
        for (const skill of shared) {
            const there = `${library}/${skill.name}/`;
            const run =
                skill.usage === ""
                    ? ""
                    : ` Run: \`${skill.usage.replaceAll(`skills/${skill.name}/`, there)}\``;
            text += `- ${skill.name}: ${skill.description}${run} (${there}SKILL.md)\n`;
        }
    }
    return text.trimEnd();
}

function cut(text: string, most: number, note: string, longer = false): string {
    const characters = [...text];
    if (characters.length <= most && !longer) return text;
    return `${characters.slice(0, most).join("")}\n${note}`;
}

async function listed(
    sources: PromptSources,
    path: string,
    context: Context,
): Promise<readonly FileEntry[] | undefined> {
    const listing = await sources.files.list(path, context);
    if (listing.ok) return listing.value;
    if (listing.error.code === "not_found") return [];
    sources.report(`${path}/ was not listed for the prompt`, listing.error);
    return undefined;
}

async function readText(
    files: AgentFilesystem,
    path: string,
    context: Context,
): Promise<string | undefined> {
    const read = await files.read(path, context);
    return read.ok ? new TextDecoder().decode(read.value) : undefined;
}

/** A field of a front matter, as `field` in `crates/hamlet_server/src/knowledge/library.rs` reads one. */
export function field(paper: string, key: string): string {
    const head = paper.startsWith("---") ? paper.slice(3) : "";
    for (const line of (head.split("\n---")[0] ?? "").split("\n")) {
        const trimmed = line.trim();
        if (trimmed.startsWith(key)) {
            return trimmed
                .slice(key.length)
                .trim()
                .replace(/^"+|"+$/g, "");
        }
    }
    return "";
}
