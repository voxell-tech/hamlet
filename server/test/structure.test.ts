import { readdirSync, readFileSync } from "node:fs";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";

const root = fileURLToPath(new URL("../src/", import.meta.url));
const MODULES: readonly { name: string; folder: boolean }[] = [
    { name: "agents", folder: true },
    { name: "change", folder: false },
    { name: "chats", folder: true },
    { name: "cli", folder: true },
    { name: "commands", folder: false },
    { name: "connection", folder: false },
    { name: "environment", folder: true },
    { name: "gate", folder: true },
    { name: "hand", folder: true },
    { name: "home", folder: true },
    { name: "house", folder: true },
    { name: "letters", folder: true },
    { name: "library", folder: true },
    { name: "limits", folder: true },
    { name: "models", folder: true },
    { name: "oversight", folder: true },
    { name: "prompt", folder: true },
    { name: "protocol", folder: false },
    { name: "realm", folder: true },
    { name: "record", folder: false },
    { name: "result", folder: false },
    { name: "sandbox", folder: true },
    { name: "say", folder: false },
    { name: "scrub", folder: true },
    { name: "server", folder: true },
    { name: "settings", folder: false },
    { name: "spend", folder: true },
    { name: "state", folder: true },
    { name: "tools", folder: true },
    { name: "transcript", folder: true },
    { name: "votes", folder: false },
];

function files(dir: string): string[] {
    return readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
        entry.isDirectory()
            ? files(resolve(dir, entry.name))
            : entry.name.endsWith(".ts")
              ? [resolve(dir, entry.name)]
              : [],
    );
}

test("concept internals are reached through their entry", () => {
    expect(
        readdirSync(root, { withFileTypes: true })
            .filter((entry) => entry.isDirectory() && entry.name !== "git")
            .map((entry) => entry.name)
            .sort(),
    ).toEqual(
        MODULES.filter((module) => module.folder).map((module) => module.name),
    );
    const failures: string[] = [];
    for (const file of files(root)) {
        const text = readFileSync(file, "utf8");
        if (file.endsWith(`${sep}index.ts`) || /\bexport\s+\*/.test(text))
            failures.push(
                `${relative(root, file)}: use named entries and re-exports`,
            );
        for (const match of text.matchAll(
            /\b(import|export)\s+(type\s+)?(\{[^}]*\}|\*\s+as\s+\w+|\w+)\s+from\s+["']([^"']+)["']/g,
        )) {
            const [, operation, type, bindings, path] = match;
            if (!path?.startsWith(".")) continue;
            const target = resolve(dirname(file), path);
            // The confined helper loads only the source files pinned in FILE_READS.
            if (
                file === resolve(root, "sandbox/file-helper.ts") &&
                target === resolve(root, "house/skill-paper.ts")
            )
                continue;
            for (const module of MODULES) {
                const folder = resolve(root, module.name) + sep;
                const inside = module.folder && file.startsWith(folder);
                const internal = module.folder && target.startsWith(folder);
                const ownEntry = target === resolve(root, `${module.name}.ts`);
                const typeOnly =
                    type ||
                    (bindings?.startsWith("{") &&
                        bindings
                            .slice(1, bindings.lastIndexOf("}"))
                            .split(",")
                            .every(
                                (binding) =>
                                    !binding.trim() ||
                                    binding.trim().startsWith("type "),
                            ));
                if (
                    (!inside &&
                        file !== resolve(root, `${module.name}.ts`) &&
                        internal) ||
                    (inside && ownEntry && operation === "import" && !typeOnly)
                ) {
                    failures.push(`${relative(root, file)} -> ${path}`);
                }
            }
        }
    }
    expect(failures).toEqual([]);
});
