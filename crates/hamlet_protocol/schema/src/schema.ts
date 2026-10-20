import { Type } from "typebox";
import * as protocol from "./protocol.ts";

/** Protocol v0 as one JSON Schema document, where each exported shape is defined once and referred to. */
export function protocolSchema(): Record<string, unknown> {
    const definitions = Object.entries(protocol).filter(([, value]) =>
        Type.IsSchema(value),
    );
    const names = new Map<string, string>();
    for (const [name, schema] of definitions) {
        const key = JSON.stringify(schema);
        const other = names.get(key);
        if (other !== undefined)
            throw new Error(`${name} and ${other} are the same shape.`);
        names.set(key, name);
    }
    const refer = (node: unknown, own = false): unknown => {
        if (typeof node !== "object" || node === null) return node;
        const name = own ? undefined : names.get(JSON.stringify(node));
        if (name !== undefined) return { $ref: `#/$defs/${name}` };
        if (Array.isArray(node)) return node.map((item) => refer(item));
        return Object.fromEntries(
            Object.entries(node).map(([key, value]) => [key, refer(value)]),
        );
    };
    return {
        $schema: "https://json-schema.org/draft/2020-12/schema",
        title: "Hamlet protocol v0",
        anyOf: [
            { $ref: "#/$defs/ClientMessage" },
            { $ref: "#/$defs/ServerMessage" },
        ],
        $defs: Object.fromEntries(
            definitions.map(([name, schema]) => [name, refer(schema, true)]),
        ),
    };
}
