import { mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import * as ts from "typescript/unstable/ast";
import { API } from "typescript/unstable/sync";
import { expect, test } from "vitest";

const root = fileURLToPath(new URL("../src/", import.meta.url));
const LEAVES = new Set([
    "chats/answers.ts",
    "chats/instructions.ts",
    "chats/notice.ts",
    "chats/tasks.ts",
    "chats/turn.ts",
    "hand/manifests.ts",
    "gate/grants.ts",
    "prompt/character.ts",
    "record/document.ts",
    "record/names.ts",
    "record/roads.ts",
    "home/abort.ts",
    "home/clock.ts",
    "home/credentials.ts",
    "home/layout.ts",
    "house/skill-paper.ts",
    "house/skills.ts",
    "library/shelf.ts",
    "limits/rules.ts",
    "models/endpoints.ts",
    "oversight/asked.ts",
    "oversight/waits.ts",
    "oversight/words.ts",
    "realm/access.ts",
    "realm/places.ts",
    "realm/roads.ts",
    "sandbox/policy.ts",
    "sandbox/reach.ts",
    "spend/ledger.ts",
    "spend/money.ts",
    "spend/usage.ts",
]);
const SHARED_ENTRIES = new Set([
    "change.ts",
    "commands.ts",
    "connection.ts",
    "protocol.ts",
    "record.ts",
    "result.ts",
    "sandbox.ts",
    "scrub.ts",
    "settings.ts",
]);

const SHARED_CORE = new Set([
    "chats/conversations.ts",
    "chats/errands.ts",
    "chats/roles.ts",
    "models/choice.ts",
]);
const ALLOWED_CYCLES = [
    // Conversation configuration deepening (D3) removes this shared core's cycle.
    [...SHARED_CORE],
    // Record deepening (D4) owns departure and merge invariants together.
    ["agents/leaving.ts", "agents/merging.ts"],
].map((component) => component.sort());
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
    { name: "record", folder: true },
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

type Dependency = {
    path: string;
    typeOnly: boolean;
};

function dependencies(source: ts.SourceFile): Dependency[] {
    const found: Dependency[] = [];
    function visit(node: ts.Node): void {
        if (
            ts.isImportDeclaration(node) &&
            ts.isStringLiteral(node.moduleSpecifier)
        ) {
            found.push({
                path: node.moduleSpecifier.text,
                typeOnly:
                    node.importClause?.phaseModifier ===
                    ts.SyntaxKind.TypeKeyword,
            });
        } else if (
            ts.isExportDeclaration(node) &&
            node.moduleSpecifier &&
            ts.isStringLiteral(node.moduleSpecifier)
        ) {
            found.push({
                path: node.moduleSpecifier.text,
                typeOnly: node.isTypeOnly,
            });
        } else if (
            ts.isImportEqualsDeclaration(node) &&
            ts.isExternalModuleReference(node.moduleReference) &&
            ts.isStringLiteral(node.moduleReference.expression)
        ) {
            found.push({
                path: node.moduleReference.expression.text,
                typeOnly: node.isTypeOnly,
            });
        } else if (
            ts.isCallExpression(node) &&
            node.expression.kind === ts.SyntaxKind.ImportKeyword
        ) {
            const path = node.arguments[0];
            if (!path || !ts.isStringLiteral(path))
                throw new Error("Use a literal path for dynamic imports");
            found.push({ path: path.text, typeOnly: false });
        }
        node.forEachChild(visit);
    }
    visit(source);
    return found;
}

function withSources<T>(dir: string, use: (sources: ts.SourceFile[]) => T): T {
    const api = new API();
    const config = resolve(dir, "../tsconfig.json");
    const snapshot = api.updateSnapshot({ openProjects: [config] });
    try {
        const project = snapshot.getProject(config);
        if (!project) throw new Error("TypeScript project is missing");
        return use(
            files(dir).map((file) => {
                const source = project.program.getSourceFile(file);
                if (!source) throw new Error(`Source file is missing: ${file}`);
                return source;
            }),
        );
    } finally {
        snapshot.dispose();
        api.close();
    }
}

function graphOf(sources: ts.SourceFile[]): Graph {
    const graph = new Map<string, string[]>();
    for (const source of sources) {
        graph.set(
            relative(root, source.fileName),
            dependencies(source)
                .filter(
                    ({ path, typeOnly }) => path.startsWith(".") && !typeOnly,
                )
                .map(({ path }) =>
                    relative(root, resolve(dirname(source.fileName), path)),
                ),
        );
    }
    for (const [file, edges] of graph) {
        for (const target of edges)
            if (!graph.has(target))
                throw new Error(
                    `Unresolved source dependency: ${file} -> ${target}`,
                );
    }
    return graph;
}

type Graph = ReadonlyMap<string, readonly string[]>;

function cycles(graph: Graph): string[][] {
    const seen = new Map<
        string,
        { index: number; low: number; active: boolean }
    >();
    const stack: string[] = [];
    const components: string[][] = [];
    function visit(file: string) {
        const node = { index: seen.size, low: seen.size, active: true };
        seen.set(file, node);
        stack.push(file);
        for (const target of graph.get(file) ?? []) {
            let next = seen.get(target);
            if (!next) {
                next = visit(target);
                node.low = Math.min(node.low, next.low);
            } else if (next.active) {
                node.low = Math.min(node.low, next.index);
            }
        }
        if (node.low === node.index) {
            const component: string[] = [];
            let member: string | undefined;
            do {
                member = stack.pop();
                if (member === undefined)
                    throw new Error("Cycle stack is empty");
                const entry = seen.get(member);
                if (entry) entry.active = false;
                component.push(member);
            } while (member !== file);
            if (component.length > 1 || graph.get(file)?.includes(file))
                components.push(component.sort());
        }
        return node;
    }
    for (const file of graph.keys()) if (!seen.has(file)) visit(file);
    return components.sort((a, b) => a.join().localeCompare(b.join()));
}

function leafFailures(graph: Graph): string[] {
    const failures: string[] = [];
    const pending = [...LEAVES, ...SHARED_CORE];
    const seen = new Set<string>();
    while (pending.length > 0) {
        const name = pending.pop();
        if (name === undefined || seen.has(name)) continue;
        seen.add(name);
        for (const target of graph.get(name) ?? []) {
            const allowed =
                LEAVES.has(target) ||
                SHARED_CORE.has(target) ||
                SHARED_ENTRIES.has(target) ||
                [...SHARED_ENTRIES].some((entry) =>
                    target.startsWith(`${entry.slice(0, -3)}/`),
                );
            if (!allowed)
                failures.push(`${name}: dependency leaf reaches ${target}`);
            pending.push(target);
        }
    }
    return failures;
}

test("concept internals are reached through entries or explicit dependency leaves", () => {
    expect(
        readdirSync(root, { withFileTypes: true })
            .filter((entry) => entry.isDirectory() && entry.name !== "git")
            .map((entry) => entry.name)
            .sort(),
    ).toEqual(
        MODULES.filter((module) => module.folder).map((module) => module.name),
    );
    const failures: string[] = [];
    withSources(root, (sources) => {
        for (const source of sources) {
            const name = relative(root, source.fileName);
            if (
                name.endsWith(`${sep}index.ts`) ||
                source.statements.some(
                    (node) =>
                        ts.isExportDeclaration(node) && !node.exportClause,
                )
            )
                failures.push(`${name}: use named entries and re-exports`);
            for (const { path, typeOnly } of dependencies(source)) {
                if (!path.startsWith(".")) continue;
                const target = relative(
                    root,
                    resolve(dirname(source.fileName), path),
                );
                const shared = LEAVES.has(target) || SHARED_CORE.has(target);
                const sameCycle = ALLOWED_CYCLES.some(
                    (cycle) => cycle.includes(name) && cycle.includes(target),
                );
                for (const module of MODULES) {
                    const folder = `${module.name}${sep}`;
                    const inside = module.folder && name.startsWith(folder);
                    const internal = module.folder && target.startsWith(folder);
                    const ownEntry = target === `${module.name}.ts`;
                    if (
                        (!inside &&
                            name !== `${module.name}.ts` &&
                            internal &&
                            !shared &&
                            !sameCycle) ||
                        (inside && ownEntry && !typeOnly)
                    )
                        failures.push(`${name} -> ${path}`);
                }
                if (
                    !typeOnly &&
                    target === "home.ts" &&
                    name !== "cli/serving.ts"
                )
                    failures.push(
                        `${name}: only server startup value-imports home.ts`,
                    );
            }
        }
    });
    failures.push(
        ...withSources(root, (sources) => leafFailures(graphOf(sources))),
    );
    expect(failures).toEqual([]);
});

test("dependency leaves cannot load composed entries through shared internals", () => {
    const graph = new Map([
        ["house/skills.ts", ["sandbox.ts"]],
        ["sandbox.ts", ["sandbox/env.ts"]],
        ["sandbox/env.ts", ["scrub.ts", "models.ts"]],
        ["scrub.ts", ["scrub/output.ts"]],
        ["scrub/output.ts", []],
        ["models.ts", []],
    ]);
    expect(leafFailures(graph)).toEqual([
        "sandbox/env.ts: dependency leaf reaches models.ts",
    ]);
});

test("value-import cycles stay within the explicit allowlist", () => {
    const components = withSources(root, (sources) => cycles(graphOf(sources)));
    expect(
        components.filter(
            (component) =>
                !ALLOWED_CYCLES.some(
                    (allowed) => allowed.join() === component.join(),
                ),
        ),
    ).toEqual([]);
});

test("cycle detection includes entry round trips and self imports", () => {
    expect(
        cycles(
            new Map([
                ["entry.ts", ["part.ts"]],
                ["part.ts", ["other.ts"]],
                ["other.ts", ["entry.ts", "leaf.ts"]],
                ["leaf.ts", []],
                ["self.ts", ["self.ts"]],
                ["consumer.ts", ["leaf.ts"]],
            ]),
        ),
    ).toEqual([["entry.ts", "other.ts", "part.ts"], ["self.ts"]]);
});

test("dependency edges follow Node's type stripping, including re-exports", () => {
    const dir = mkdtempSync(resolve(tmpdir(), "hamlet-structure-"));
    try {
        writeFileSync(
            resolve(dir, "tsconfig.json"),
            JSON.stringify({ include: ["*.ts"] }),
        );
        writeFileSync(
            resolve(dir, "fixture.ts"),
            `
            import type { Home } from "./type-import.ts";
            export type { Home } from "./type-export.ts";
            type Wire = import("./type-query.ts").Wire;
            import Plain from "./default.ts";
            import Mixed, { value } from "./mixed.ts";
            import * as space from "./namespace.ts";
            import "./side-effect.ts";
            import { type Shape } from "./inline-type.ts";
            export { value } from "./re-export.ts";
            export { type Shape } from "./inline-export.ts";
            export * from "./wildcard.ts";
            const later = () => import("./dynamic.ts");
            // import { fake } from "./comment.ts";
            const text = 'export { fake } from "./string.ts"';
        `,
        );
        const api = new API();
        const config = resolve(dir, "tsconfig.json");
        const snapshot = api.updateSnapshot({ openProjects: [config] });
        try {
            const source = snapshot
                .getProject(config)
                ?.program.getSourceFile(resolve(dir, "fixture.ts"));
            if (!source) throw new Error("Dependency fixture is missing");
            const found = dependencies(source);
            expect(
                found.filter((edge) => !edge.typeOnly).map((edge) => edge.path),
            ).toEqual([
                "./default.ts",
                "./mixed.ts",
                "./namespace.ts",
                "./side-effect.ts",
                "./inline-type.ts",
                "./re-export.ts",
                "./inline-export.ts",
                "./wildcard.ts",
                "./dynamic.ts",
            ]);
            expect(
                found.filter((edge) => edge.typeOnly).map((edge) => edge.path),
            ).toEqual(["./type-import.ts", "./type-export.ts"]);
        } finally {
            snapshot.dispose();
            api.close();
        }
    } finally {
        rmSync(dir, { recursive: true, force: true });
    }
});
