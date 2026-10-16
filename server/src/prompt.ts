import { join } from "node:path";
import type { Context } from "@earendil-works/chord";
import {
    defineExtension,
    type PromptInput,
    section,
} from "@earendil-works/pi-durable";
import type { With } from "./chats/conversations.ts";
import { agentOf } from "./chats/errands.ts";
import { WorkDoc } from "./environment.ts";
import { ManifestsDoc, manifestFaultText } from "./hand/manifests.ts";
import { houseOf } from "./home/layout.ts";
import type { Home, Part } from "./home.ts";
import { installStarters, libraryPart } from "./library/shelf.ts";
import { CharacterDoc } from "./prompt/character.ts";
import { charterOf } from "./prompt/charter.ts";
import { agentKey } from "./record.ts";
import { modelName, SettingsDoc, settingsOf } from "./settings.ts";

/** The machine's shell tools, as the prompt names them (prompt.rs:10-17). */
const SYSTEM =
    process.platform === "darwin"
        ? "System: macOS, whose shell tools are the BSD ones. Flags that only GNU has, such as `grep -P`, `cat -A` and `sed -r`, fail."
        : "System: Linux.";

/** Characters of NOTES.md, and of a project's own instructions, a prompt shows. */
const NOTES_SHOWN = 8000;

/** Characters of a character a prompt shows. */
const CHARACTER_SHOWN = 4000;

/** The files that hold a project's own instructions for agents, the first that reads winning. */
const INSTRUCTIONS = ["AGENTS.md", "CLAUDE.md"];

/** The input of a part of an agent's prompt. */
type PartInput = {
    agent: With;
    /** Its house, its whole writable workspace. */
    house: string;
    input: PromptInput;
    context: Context;
};

/**
 * The parts every agent is told before each request, in C11's order under Rust's headings: the charter, the machine,
 * the project, the character, the notes, the skills and the library. Its start stocks the library with the starters.
 */
export const Prompt: Part = (home) => {
    // A part of a conversation with no agent is left out.
    const part = (
        key: string,
        render: (given: PartInput) => Promise<string | undefined>,
    ) =>
        section(
            key,
            async (input, context) => {
                const agent = await agentOf(
                    input.read,
                    input.conversationId,
                    context,
                );
                if (agent === undefined) return undefined;
                const house = houseOf(home.layout, agent);
                return render({ agent, house, input, context });
            },
            { tag: false },
        );
    return {
        extension: defineExtension({
            name: "hamlet.prompt",
            sections: [
                part("charter", async ({ input, context }) =>
                    (await charterOf(input.read, context)).trimEnd(),
                ),
                part("machine", (given) => machine(home, given)),
                part("project", project),
                part("character", character),
                part("notes", notes),
                part("skills", (given) => skills(home, given)),
                part("library", () => libraryPart(home.layout.library)),
            ],
        }),
        async start() {
            try {
                await installStarters(home.layout.library);
            } catch (error) {
                const text =
                    error instanceof Error ? error.message : String(error);
                process.stderr.write(
                    `hamlet: could not open the library: ${text}\n`,
                );
            }
        },
    };
};

/** The sandbox, the network, the model, the tools that failed to load, the OS and the workspace (worker.rs:41-56). */
async function machine(
    home: Home,
    { house, input, context }: PartInput,
): Promise<string> {
    const settings = settingsOf(
        await input.read.snapshot(SettingsDoc, context),
        home.overrides,
    );
    // A chief oversees every call, so a call that needs the network waits.
    const net = settings.allowNet
        ? "on"
        : "off unless a chief allows it; a command that needs it is held until the chief answers, so set `network` to true on it and carry on";
    const model =
        input.agent.model === undefined
            ? ""
            : ` Model: ${modelName(input.agent.model)}.`;
    const faults = (await input.read.snapshot(ManifestsDoc, context))?.faults;
    const failed = faults?.length
        ? `\nTools that failed to load:\n${home.secrets.words(faults.map(manifestFaultText).join("\n"))}`
        : "";
    return `# Environment\nSandbox: ${home.sandbox.name}. Network for tools: ${net}.${model}${failed}\n${SYSTEM}\nWorkspace: ${house}`;
}

/** The folder the work is done in while the hamlet works on one, with the folder's own instructions (prompt.rs:39-93). */
async function project({
    agent,
    house,
    input,
    context,
}: PartInput): Promise<string | undefined> {
    const work = await input.read.snapshot(WorkDoc, agent.hamlet, context);
    const folder = work?.folder;
    if (work === undefined || folder == null) return undefined;
    const lines = [
        `Your hamlet works on the folder ${folder}. The work a chief asks for is done there. This changes what is said above about where commands run:`,
        `- \`bash\` runs with ${folder} as its working directory. A command may write there and in your own workspace, and nowhere else. It can read the rest of the machine, so the project's own tools (cargo, npm, make and the like) run as they do for the chief.`,
        "- `read`, `write` and `edit` take a path relative to the project, or a full path.",
        "- `read`, `search` and `find` look at files without running a command. Use them rather than cat, grep and find.",
        `- Your own workspace is ${house}. NOTES.md and skills/ are there and stay there: never put notes, skills or files of your own in the project. Give \`write\` and \`edit\` their full path, and run a skill from your workspace, as shown below.`,
    ];
    const git = await input.env?.exists(join(folder, ".git"), context);
    if (git?.ok && git.value) {
        lines.push(
            "- The project is a git repository. When two agents must work apart, the mayor may have one of them use a git worktree inside the project folder, for example under `.hamlet/worktrees/<name>`: only the project folder and an agent's own workspace can be written.",
        );
    }
    if (work.mode === "manual") {
        lines.push(
            "- The chief approves each edit and each command before it runs, so each one waits. `read`, `search` and `find` never wait: learn what you need with them, then make few, well-aimed edits and commands.",
        );
    }
    for (const name of INSTRUCTIONS) {
        const read = await input.env?.readTextFile(join(folder, name), context);
        if (!read?.ok) continue;
        const note = `\n[${name} is longer than ${NOTES_SHOWN} characters and is cut here.]`;
        lines.push(
            `\n# The project's ${name}\n${cut(read.value, NOTES_SHOWN, note)}`,
        );
        break;
    }
    return `# The project\n${lines.join("\n")}`.trimEnd();
}

/** The agent's character, from its document; none while it is blank (prompt.rs:112-122). */
async function character({
    agent,
    input,
    context,
}: PartInput): Promise<string | undefined> {
    const key = agentKey(agent);
    const text = (
        await input.read.snapshot(CharacterDoc, key, context)
    )?.text.trim();
    if (!text) return undefined;
    const note = `\n[The character is longer than ${CHARACTER_SHOWN} characters and is cut here.]`;
    return `# Your character\n${cut(text, CHARACTER_SHOWN, note)}`;
}

/** The agent's NOTES.md, the heading alone when it cannot be read (prompt.rs:132-143). */
async function notes({ house, input, context }: PartInput): Promise<string> {
    const read = await input.env?.readTextFile(
        join(house, "NOTES.md"),
        context,
    );
    const note = `\n[NOTES.md is longer than ${NOTES_SHOWN} characters and is cut here. Shorten it so that all of it is shown.]`;
    const text = read?.ok ? cut(read.value, NOTES_SHOWN, note) : "";
    return `# NOTES.md\n${text}`.trimEnd();
}

/** The house's skills, descriptions and commands starting in its house (C8). */
async function skills(
    home: Home,
    { agent, house, context }: PartInput,
): Promise<string> {
    const found = await home.houses.of(agent).skills(context);
    const lines = found.map(({ name, description, usage, folder }) => {
        const run =
            usage === undefined
                ? ""
                : ` Run: \`cd '${house.replaceAll("'", "'\\''")}' && ${usage}\``;
        return `- ${name}: ${description}${run} (skills/${folder}/SKILL.md)`;
    });
    return `# Skills\n${lines.length === 0 ? "(none yet)" : lines.join("\n")}`;
}

/** The first `most` characters of `text`, with `note` after them where it is cut. */
function cut(text: string, most: number, note: string): string {
    const characters = [...text];
    return characters.length > most
        ? `${characters.slice(0, most).join("")}${note}`
        : text;
}

export { HamletBrief } from "./prompt/brief.ts";
export { CharacterDoc } from "./prompt/character.ts";
export {
    CharterDoc,
    charterOf,
    DEFAULT_CHARTER,
    editCharter,
} from "./prompt/charter.ts";
export { told } from "./prompt/told.ts";
