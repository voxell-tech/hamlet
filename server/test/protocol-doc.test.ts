import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

it("documents every client and server message type in protocol v0", () => {
    const schema: unknown = JSON.parse(
        readFileSync(
            new URL("../protocol.schema.json", import.meta.url),
            "utf8",
        ),
    );
    if (
        typeof schema !== "object" ||
        schema === null ||
        !("$defs" in schema) ||
        typeof schema.$defs !== "object" ||
        schema.$defs === null
    )
        throw new Error("The protocol schema must have definitions.");
    const definitions = new Map<string, unknown>(Object.entries(schema.$defs));
    const types = (node: unknown): string[] => {
        if (typeof node !== "object" || node === null)
            throw new Error("Expected a message schema.");
        if ("$ref" in node && typeof node.$ref === "string")
            return types(definitions.get(node.$ref.replace("#/$defs/", "")));
        if ("anyOf" in node && Array.isArray(node.anyOf))
            return node.anyOf.flatMap(types);
        if (
            "properties" in node &&
            typeof node.properties === "object" &&
            node.properties !== null &&
            "type" in node.properties &&
            typeof node.properties.type === "object" &&
            node.properties.type !== null &&
            "const" in node.properties.type &&
            typeof node.properties.type.const === "string"
        )
            return [node.properties.type.const];
        throw new Error("Expected a message type literal.");
    };
    const page = readFileSync(
        new URL("../../docs/protocol.md", import.meta.url),
        "utf8",
    );
    const missing = ["ClientMessage", "ServerMessage"].flatMap((union) => {
        const names = [...new Set(types(definitions.get(union)))];
        expect(names.length).toBeGreaterThan(0);
        return names
            .filter((name) => !page.includes(`\`${name}\``))
            .map((name) => `${union}: ${name}`);
    });
    expect(missing, "Message types missing from docs/protocol.md").toEqual([]);
});
