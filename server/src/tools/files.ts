import type { JsonValue } from "@earendil-works/chord";
import type { TSchema } from "@earendil-works/pi-ai";
import type { ToolRegistration } from "@earendil-works/pi-durable";
import {
    createEditTool,
    createReadTool,
    createWriteTool,
} from "@earendil-works/pi-durable/tools";
import { placeOf, remoteFooting } from "../environment.ts";
import type { Home } from "../home.ts";

/** pi's `read`, `write` and `edit`, whose paths fall in the house or the folder as C20 says. */
export function fileTools(home: Home) {
    return [
        footed(home, createReadTool()),
        footed(home, createWriteTool()),
        footed(home, createEditTool()),
    ];
}

/** Resolves paths in the tool's own filesystem before invoking pi. */
function footed<P extends TSchema, D extends JsonValue>(
    home: Home,
    tool: ToolRegistration<P, D>,
): ToolRegistration<P, D> {
    return {
        ...tool,
        execute: async (args, api, context) => {
            const place = await placeOf(home, api, api.conversationId, context);
            const { path } = args as { path: string };
            if (place === undefined || api.env === undefined)
                return tool.execute(args, api, context);
            const resolved = await remoteFooting(path, place, api.env, context);
            if (!resolved.ok)
                return {
                    isError: true,
                    output: [{ type: "text", text: resolved.error.message }],
                };
            return tool.execute(
                { ...args, path: resolved.value },
                api,
                context,
            );
        },
    };
}
