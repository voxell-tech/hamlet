import { defineExtension, hook, ToolTask } from "@earendil-works/pi-durable";
import type { Part } from "./home.ts";
import { fileTools } from "./tools/files.ts";
import { Find } from "./tools/find.ts";
import { Search } from "./tools/search.ts";
import { bashTool } from "./tools/shell.ts";

/** The tools every agent works with, whether or not its hamlet works on a folder (C16). */
export const Tools: Part = (home) => ({
    extension: defineExtension({
        name: "hamlet.tools",
        tools: [...fileTools(home), bashTool(home), Search, Find],
        hooks: [
            hook(ToolTask, {
                afterTool: (_call, result) => home.secrets.value(result),
            }),
        ],
    }),
});

export {
    type RunFailure,
    type SearchFailure,
    type ToolFailure,
    toolDefect,
    toolFailureText,
} from "./tools/failure.ts";
export { leaveFor, runConfined } from "./tools/run.ts";
export { bashParameters } from "./tools/shell.ts";
