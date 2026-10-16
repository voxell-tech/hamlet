import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import * as ts from "typescript/unstable/ast";
import { API } from "typescript/unstable/sync";

/** Files whose exported declarations still need documentation. */
const ALLOWLIST = new Set<string>([
    "commands.ts",
    "git/proxy.ts",
    "protocol.ts",
]);
const root = fileURLToPath(new URL("../src/", import.meta.url));

function files(dir: string): string[] {
    return readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
        entry.isDirectory()
            ? files(`${dir}${entry.name}/`)
            : entry.name.endsWith(".ts")
              ? [`${dir}${entry.name}`]
              : [],
    );
}

function modifiers(node: ts.Node): readonly ts.ModifierLike[] {
    const found: ts.ModifierLike[] = [];
    node.forEachChild((child) => {
        if (ts.isModifierLike(child)) found.push(child);
    });
    return found;
}

function documented(node: ts.Node): boolean {
    return (node.jsDoc?.length ?? 0) > 0;
}

function pair(
    node: ts.Statement,
    previous: ts.Statement | undefined,
    source: ts.SourceFile,
): boolean {
    if (
        !ts.isTypeAliasDeclaration(node) ||
        !previous ||
        !ts.isVariableStatement(previous) ||
        !documented(previous)
    )
        return false;
    if (
        !ts.isTypeReferenceNode(node.type) ||
        node.type.typeName.getText(source) !== "Static" ||
        node.type.typeArguments?.length !== 1
    )
        return false;
    const argument = node.type.typeArguments[0];
    if (
        !argument ||
        !ts.isTypeQueryNode(argument) ||
        argument.exprName.getText(source) !== node.name.text
    )
        return false;
    return (
        previous.declarationList.declarations.some(
            (declaration) =>
                ts.isIdentifier(declaration.name) &&
                declaration.name.text === node.name.text,
        ) &&
        source.getLineAndCharacterOfPosition(previous.end).line + 1 ===
            source.getLineAndCharacterOfPosition(node.getStart(source)).line
    );
}

const api = new API();
const snapshot = api.updateSnapshot({
    openProjects: [fileURLToPath(new URL("../tsconfig.json", import.meta.url))],
});
const project = snapshot.getProject(
    fileURLToPath(new URL("../tsconfig.json", import.meta.url)),
);
if (!project) throw new Error("Server TypeScript project is missing");

const failures: string[] = [];
const allowed = new Set<string>();
let missing = 0;
for (const file of files(root)) {
    const name = file.slice(root.length);
    const source = project.program.getSourceFile(file);
    if (!source) throw new Error(`Source file is missing: ${file}`);
    const nodes: ts.Node[] = [];
    for (const [index, statement] of source.statements.entries()) {
        if (
            !modifiers(statement).some(
                (modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword,
            )
        )
            continue;
        if (ts.isExportDeclaration(statement)) continue;
        if (
            !documented(statement) &&
            !pair(statement, source.statements[index - 1], source)
        )
            nodes.push(statement);
        if (ts.isClassDeclaration(statement)) {
            for (const member of statement.members) {
                if (
                    ts.isClassStaticBlockDeclaration(member) ||
                    ts.isSemicolonClassElement(member)
                )
                    continue;
                const named =
                    ts.isPropertyDeclaration(member) ||
                    ts.isMethodDeclaration(member) ||
                    ts.isGetAccessorDeclaration(member) ||
                    ts.isSetAccessorDeclaration(member);
                if (
                    (named && ts.isPrivateIdentifier(member.name)) ||
                    modifiers(member).some(
                        (modifier) =>
                            modifier.kind === ts.SyntaxKind.PrivateKeyword ||
                            modifier.kind === ts.SyntaxKind.ProtectedKeyword,
                    )
                )
                    continue;
                if (!documented(member)) nodes.push(member);
            }
        }
    }
    if (nodes.length && ALLOWLIST.has(name)) {
        allowed.add(name);
        missing += nodes.length;
    } else {
        for (const node of nodes)
            failures.push(
                `${name}:${source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1}: exported declaration needs a doc comment`,
            );
    }
}
for (const name of ALLOWLIST) {
    if (!allowed.has(name))
        failures.push(`${name}: remove the stale documentation allowance`);
}
if (failures.length) {
    console.error(failures.join("\n"));
    process.exitCode = 1;
} else {
    console.log(
        `Documentation checked; ${allowed.size} allowed files (${missing} declarations).`,
    );
}

snapshot.dispose();
api.close();
