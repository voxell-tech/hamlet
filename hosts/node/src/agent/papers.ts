/**
 * A client's read of the agent's knowledge, and its edit of the agent's
 * memory.
 */
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { type AgentCall, generationOf } from "./actor.ts";
import type { AgentFilesystem } from "./filesystem.ts";
import type { EditMemory, Papers } from "./messages/agent.ts";
import { field } from "./prompt.ts";

const MEMORY = "memory/";

const SKILL = /^skills\/([^/]+)\/SKILL\.md$/;

async function texts(
    files: AgentFilesystem,
    folder: string,
): Promise<[string, string][]> {
    const walked = await files.walk(folder, BACKGROUND_CONTEXT);
    if (!walked.ok) {
        if (walked.error.code === "not_found") return [];
        throw walked.error;
    }
    const found: [string, string][] = [];
    for (const path of [...walked.value.files].sort()) {
        const read = await files.read(path, BACKGROUND_CONTEXT);
        if (read.ok) found.push([path, new TextDecoder().decode(read.value)]);
    }
    return found;
}

export async function readPapers(c: AgentCall): Promise<Papers> {
    const { peers, branch } = generationOf(c);
    const learned = await branch.learned();
    const memory = (await texts(peers.files, "memory")).map(([path, text]) => ({
        description: field(text, "description:"),
        path: path.slice(MEMORY.length),
        text,
    }));
    const skills = (await texts(peers.files, "skills")).flatMap(
        ([path, text]) => {
            const name = SKILL.exec(path)?.[1];
            return name === undefined
                ? []
                : [{ description: field(text, "description:"), name }];
        },
    );
    return {
        character: learned?.character ?? "",
        learned: learned?.paths.map(({ path }) => path) ?? [],
        memory,
        skills,
    };
}

function inMemory(path: string): boolean {
    return (
        path.startsWith(MEMORY) &&
        path
            .slice(MEMORY.length)
            .split("/")
            .every((part) => part !== "" && part !== "." && part !== "..")
    );
}

/** Writes the memory file, or removes it for empty text, then saves. */
export async function editMemory(
    c: AgentCall,
    { path, text }: EditMemory,
): Promise<null> {
    if (!inMemory(path)) throw new Error(`${path} is not a file under memory/`);
    const { peers, branch } = generationOf(c);
    const done =
        text === ""
            ? await peers.files.remove(path, BACKGROUND_CONTEXT)
            : await peers.files.write(path, text, BACKGROUND_CONTEXT);
    if (!done.ok && done.error.code !== "not_found") throw done.error;
    await branch.save();
    return null;
}
