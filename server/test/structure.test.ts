import { readdirSync, readFileSync } from "node:fs";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";

const root = fileURLToPath(new URL("../src/", import.meta.url));
const MODULES: readonly { name: string; folder: boolean }[] = [
    { name: "oversight", folder: true },
    { name: "gate", folder: true },
    { name: "environment", folder: true },
    { name: "hand", folder: true },
    { name: "tools", folder: true },
    { name: "house", folder: true },
    { name: "library", folder: true },
    { name: "scrub", folder: true },
    { name: "sandbox", folder: true },
    { name: "record", folder: false },
    { name: "realm", folder: true },
    { name: "home", folder: true },
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
    const failures: string[] = [];
    for (const file of files(root)) {
        const text = readFileSync(file, "utf8");
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
