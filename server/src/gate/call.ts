import type { ToolHookCall } from "@earendil-works/pi-durable";
import {
    createEditTool,
    createWriteTool,
} from "@earendil-works/pi-durable/tools";
import { Value } from "typebox/value";
import type { Manifest } from "../hand/manifests.ts";
import { bashParameters } from "../tools.ts";

/** A validated tool call, classified by its reach. */
export type Call =
    | { kind: "shell"; command: string; net: boolean }
    | {
          kind: "edit";
          source: "write" | "edit";
          path: string;
          old: string;
          now: string;
          created: boolean;
      }
    | { kind: "read" }
    | {
          kind: "custom";
          net: boolean;
          name: string;
          args: ToolHookCall["arguments"];
      }
    | { kind: "society"; name: string; args: ToolHookCall["arguments"] };

const write = createWriteTool().parameters;
const edit = createEditTool().parameters;

/** Narrows the harness-validated arguments with the tool's own schema; a mismatch is a defect. */
export function callOf(
    call: ToolHookCall,
    manifests: readonly Manifest[],
): Call {
    if (call.name === "bash") {
        if (!Value.Check(bashParameters, call.arguments))
            throw new Error("Invalid validated bash call");
        return {
            kind: "shell",
            command: call.arguments.command,
            net: call.arguments.network === true,
        };
    }
    if (call.name === "write") {
        if (!Value.Check(write, call.arguments))
            throw new Error("Invalid validated write call");
        return {
            kind: "edit",
            source: "write",
            path: call.arguments.path,
            old: "",
            now: call.arguments.content,
            created: true,
        };
    }
    if (call.name === "edit") {
        if (!Value.Check(edit, call.arguments))
            throw new Error("Invalid validated edit call");
        const old = call.arguments.edits.map((edit) => edit.oldText).join("\n");
        return {
            kind: "edit",
            source: "edit",
            path: call.arguments.path,
            old,
            now: call.arguments.edits.map((edit) => edit.newText).join("\n"),
            created: old === "",
        };
    }
    if (["read", "search", "find"].includes(call.name)) return { kind: "read" };
    const manifest = manifests.find((tool) => tool.name === call.name);
    return manifest === undefined
        ? { kind: "society", name: call.name, args: call.arguments }
        : {
              kind: "custom",
              net: manifest.net,
              name: call.name,
              args: call.arguments,
          };
}

/** The bounded subject a chief sees when an approval waits. */
export function callText(call: Call): string {
    switch (call.kind) {
        case "shell":
            return call.command;
        case "edit":
            return call.created
                ? `${cut(call.path)}\nA new file:\n${cut(call.now)}`
                : `${cut(call.path)}\nReplace:\n${cut(call.old)}\nWith:\n${cut(call.now)}`;
        case "society":
        case "custom":
            return `${call.name} ${JSON.stringify(call.args)}`;
        case "read":
            return "";
    }
    const unhandled: never = call;
    throw new Error(`Unclassified call: ${unhandled}`);
}

function cut(text: string): string {
    const chars = [...text];
    const shown = 2000;
    return chars.length > shown
        ? `${chars.slice(0, shown).join("")}\n[${chars.length - shown} more characters]`
        : text;
}
