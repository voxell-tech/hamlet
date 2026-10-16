import { resolve } from "node:path";
import { isJsonValue } from "@earendil-works/chord";
import type { Registry } from "@earendil-works/pi-durable";
import { defineDoc, type JsonObject } from "@earendil-works/pi-durable";
import { Type } from "typebox";
import { Compile } from "typebox/compile";
import { Value } from "typebox/value";
import { err, ok, type Result } from "../result.ts";

/** A tool a chief added by hand: a program and its manifest in a folder of its own (C17). */
export type Manifest = {
    name: string;
    description: string;
    /** Its folder, which it runs from and can read. */
    dir: string;
    /** The program and its arguments. */
    exec: string[];
    /** The JSON Schema of its arguments. */
    parameters: JsonObject;
    /** True when it asks for the network. */
    net: boolean;
    /** Paths it reads, besides its folder. */
    ro: string[];
    /** Paths it writes. */
    rw: string[];
    env: { [name: string]: string };
    /** Seconds it may run, `null` for the sandbox's default. */
    wallSecs: number | null;
    /** Seconds of CPU it may use, `null` for the sandbox's default. */
    cpuSecs: number | null;
};

/** The tools added by hand that loaded, and a line for each that did not (C15). */
export const ManifestsDoc = defineDoc<{
    tools: Manifest[];
    faults: ManifestFault[];
}>({
    kind: "hamlet.manifests",
    version: 1,
    scope: "session",
    initial: () => ({ tools: [], faults: [] }),
});

/** A manifest refused before its tool is offered. */
export type ManifestFault = {
    operation: "load-tool";
    dir: string;
    retrySafe: true;
} & (
    | { kind: "not-json" }
    | { kind: "missing-name" }
    | { kind: "name-format"; name: string }
    | { kind: "exec-empty"; reason: "not-array" | "empty" }
    | { kind: "parameters-not-schema" }
    | { kind: "schema-compile"; reason: string }
    | {
          kind: "limit-not-whole";
          field: "wallSecs" | "cpuSecs";
          value: number | string;
          constraint: "whole-positive" | "timer";
      }
    | { kind: "name-reserved"; name: string }
    | { kind: "name-taken"; name: string }
    | { kind: "not-a-folder" }
    | { kind: "manifest-unreadable"; reason: string }
);

/** Manifest diagnostics; legacy stored strings remain readable without changing the document version. */
export function manifestFaultText(fault: ManifestFault | string): string {
    if (typeof fault === "string") return fault;
    return `${fault.dir}: ${reasonText(fault)}`;
}

function reasonText(fault: ManifestFault): string {
    switch (fault.kind) {
        case "not-json":
            return "manifest.json is not valid JSON";
        case "missing-name":
            return "missing name";
        case "name-format":
            return `name '${fault.name}' must match [a-zA-Z0-9_-]{1,64}`;
        case "exec-empty":
            return fault.reason === "not-array"
                ? "exec must be an array"
                : "exec is empty";
        case "parameters-not-schema":
            return "parameters do not compile: parameters must be a JSON Schema object";
        case "schema-compile":
            return `parameters do not compile: ${fault.reason}`;
        case "limit-not-whole":
            return fault.constraint === "timer"
                ? `The limit ${fault.field} is more than 2147483 seconds: ${fault.value}`
                : `The limit ${fault.field} is not a whole number of at least 1: ${fault.value}`;
        case "name-reserved":
            return `name '${fault.name}' is reserved`;
        case "name-taken":
            return `name '${fault.name}' is taken by another tool`;
        case "not-a-folder":
            return "not a folder";
        case "manifest-unreadable":
            return fault.reason;
    }
    const unhandled: never = fault;
    throw new Error(`Unclassified manifest fault: ${unhandled}`);
}

const objectInput = Type.Record(Type.String(), Type.Unknown());
const stringInput = Type.String();
const arrayInput = Type.Array(Type.Unknown());
const nameInput = Type.String({ pattern: "^[a-zA-Z0-9_-]{1,64}$" });
const secondsInput = Type.Integer({
    minimum: 1,
    maximum: Number.MAX_SAFE_INTEGER,
});

function object(value: unknown): JsonObject {
    if (isJsonValue(value) && Value.Check(objectInput, value)) return value;
    return {};
}

/** Parses and normalizes an untrusted manifest without quoting invalid JSON in diagnostics. */
export function parseManifest(
    text: string,
    dir: string,
): Result<Manifest, ManifestFault> {
    const facts = {
        operation: "load-tool" as const,
        dir,
        retrySafe: true as const,
    };
    let parsed: unknown;
    try {
        parsed = JSON.parse(text);
    } catch {
        return err({ ...facts, kind: "not-json" });
    }
    const value = object(parsed);
    if (!Value.Check(stringInput, value.name))
        return err({ ...facts, kind: "missing-name" });
    if (!Value.Check(arrayInput, value.exec))
        return err({ ...facts, kind: "exec-empty", reason: "not-array" });
    const exec = value.exec.filter((part): part is string =>
        Value.Check(stringInput, part),
    );
    if (exec.length === 0)
        return err({ ...facts, kind: "exec-empty", reason: "empty" });
    if (!Value.Check(nameInput, value.name))
        return err({ ...facts, kind: "name-format", name: value.name });
    const parameters =
        value.parameters === undefined
            ? { type: "object", properties: {} }
            : value.parameters;
    if (!isJsonValue(parameters) || !Value.Check(objectInput, parameters))
        return err({ ...facts, kind: "parameters-not-schema" });
    try {
        Compile(parameters);
    } catch (cause) {
        return err({
            ...facts,
            kind: "schema-compile",
            reason: cause instanceof Error ? cause.message : String(cause),
        });
    }
    const sandbox = object(value.sandbox);
    const limits = object(sandbox.limits);
    const parsedLimits: { wallSecs: number | null; cpuSecs: number | null } = {
        wallSecs: null,
        cpuSecs: null,
    };
    for (const [field, value] of [
        ["wallSecs", limits.wall_secs],
        ["cpuSecs", limits.cpu_secs],
    ] as const) {
        if (value === undefined) continue;
        if (!Value.Check(secondsInput, value))
            return err({
                ...facts,
                kind: "limit-not-whole",
                field,
                value: Value.Check(Type.Number(), value)
                    ? value
                    : JSON.stringify(value),
                constraint: "whole-positive",
            });
        if (field === "wallSecs" && value > 2147483)
            return err({
                ...facts,
                kind: "limit-not-whole",
                field,
                value,
                constraint: "timer",
            });
        parsedLimits[field] = value;
    }
    const paths = (value: unknown): string[] =>
        Value.Check(arrayInput, value)
            ? value
                  .filter((path): path is string =>
                      Value.Check(stringInput, path),
                  )
                  .map((path) => resolve(dir, path))
            : [];
    return ok({
        name: value.name,
        description: Value.Check(stringInput, value.description)
            ? value.description
            : "",
        dir,
        exec,
        parameters,
        net: sandbox.net === true,
        ro: [...paths(sandbox.fs_ro), dir],
        rw: paths(sandbox.fs_rw),
        env: Object.fromEntries(
            Object.entries(object(sandbox.env)).map(([name, value]) => [
                name,
                Value.Check(stringInput, value) ? value : "",
            ]),
        ),
        ...parsedLimits,
    });
}

/** Names owned by installed extensions and pi, excluding the hand loader's replaceable slot. */
export function reservedNames(registry: Registry): ReadonlySet<string> {
    return new Set([
        "bash",
        "powershell",
        "read",
        "write",
        "edit",
        ...registry
            .snapshot()
            .tools()
            .filter(({ extension }) => extension.name !== "hamlet.hand")
            .map(({ tool }) => tool.name),
    ]);
}
