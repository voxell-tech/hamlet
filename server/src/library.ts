import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { skillsIn } from "./skills.ts";

/** The starter skills the server ships, one folder each (H33). */
export const STARTERS = fileURLToPath(
    new URL("../assets/starters/", import.meta.url),
);

/** SHA-256 of every starter file as an earlier version shipped it, which nobody edited since. */
const SHIPPED = new Set([
    "058332dd952ac923df1e66ef40d86ddf09231f3ae9f0c375d438b3f54ccf5f10",
    "332b7d2c7c9dff2a539e363c1dd45ef8f0df4b0f5bec81a175776b88aaa60b05",
    "42bc7dcb52ec791f7add8d4c761beb61682dc6cbabc3f9cca5f10af0315b8760",
    "4a485c4b0a24f5897cd9a5dc201e1864e0db0bc9cf0fcc8108d912897e129a95",
    "7014db70c2c5c4e7668b32005e295221921654261eb310dbd4b957ebb21427e8",
    "7fdddf97dee72752a9ce63672afb8394e45d7c5c29a8290fe3972cfadb94b82c",
    "ad0193b00a5ec14f9676d395de721fff1458d12304c293980ab9dfe9dddb79fa",
    "e47ad9997acd2f1709c645539b58493cee503203dc6a91a72a75751311f87b14",
    "e7ff39d4d4bcfdc42e9972b78db7ccee19d8a0756aa5c63aa14a64750440eb31",
]);

/** The file in the library that names the starters it was offered, one per line. */
const RECORD = ".starters";

/**
 * Puts into the library at `root` each starter it was not offered yet, unless a folder of that name stands there, and
 * brings up to date each file of an offered one that nobody edited. A deleted starter is never put back. Gives the
 * names written (starters.rs:58-85).
 */
export async function installStarters(root: string): Promise<string[]> {
    const record = join(root, RECORD);
    const offered = existsSync(record)
        ? (await readFile(record, "utf8")).split("\n")
        : [];
    const written: string[] = [];
    for (const name of (await readdir(STARTERS)).sort()) {
        const files = await filesOf(name);
        const dir = join(root, name);
        if (offered.includes(name)) {
            for (const file of files) {
                const old = await readFile(join(dir, file), "utf8").catch(
                    () => undefined,
                );
                if (old !== undefined && SHIPPED.has(sha256(old))) {
                    await writeFile(join(dir, file), await shipped(name, file));
                }
            }
            continue;
        }
        if (!existsSync(dir)) {
            for (const file of files) {
                await mkdir(dirname(join(dir, file)), { recursive: true });
                await writeFile(join(dir, file), await shipped(name, file));
            }
            written.push(name);
        }
        offered.push(name);
    }
    const names = offered.filter((name) => name !== "");
    await writeFile(record, names.map((name) => `${name}\n`).join(""));
    return written;
}

/** The library's part of a prompt: where it is, and each skill with the command that runs it from there (library.rs:98-145). */
export async function libraryPart(root: string): Promise<string | undefined> {
    const skills = await skillsIn(root);
    if (skills.length === 0) return undefined;
    const lines = skills.map(({ name, description, usage, folder }) => {
        const there = `${root}/${folder}/`;
        const run =
            usage === undefined
                ? ""
                : ` Run: \`${usage.replaceAll(`skills/${folder}/`, there)}\``;
        return `- ${name}: ${description}${run} (${there}SKILL.md)`;
    });
    return [
        `# Library\nSkills every agent shares, kept in ${root}. Use one as it stands: read it and run it from there, and do not copy it into your workspace. Where its text says \`skills/<name>/\`, its files are in \`${root}/<name>/\`. You cannot write there.`,
        ...lines,
    ].join("\n");
}

/** The files of the starter `name`, relative to its folder. */
async function filesOf(name: string): Promise<string[]> {
    const entries = await readdir(join(STARTERS, name), {
        recursive: true,
        withFileTypes: true,
    });
    return entries
        .filter((entry) => entry.isFile())
        .map((entry) =>
            join(entry.parentPath, entry.name).slice(
                join(STARTERS, name).length + 1,
            ),
        )
        .sort();
}

function shipped(name: string, file: string): Promise<string> {
    return readFile(join(STARTERS, name, file), "utf8");
}

function sha256(text: string): string {
    return createHash("sha256").update(text).digest("hex");
}
