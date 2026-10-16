import type { JsonValue } from "@earendil-works/chord";
import type { TSchema } from "@earendil-works/pi-ai";
import type { ToolRegistration } from "@earendil-works/pi-durable";
import {
    createEditTool,
    createReadTool,
    createWriteTool,
} from "@earendil-works/pi-durable/tools";
import { footing, placeOf } from "../environment.ts";
import type { Home } from "../home.ts";

/** pi's `read`, `write` and `edit`, whose paths fall in the house or the folder as C20 says. */
export function fileTools(home: Home) {
    return [
        footed(home, createReadTool()),
        footed(home, createWriteTool()),
        footed(home, createEditTool()),
    ];
}

/** `tool`, given its `path` as {@link footing} puts it. */
function footed<P extends TSchema, D extends JsonValue>(
    home: Home,
    tool: ToolRegistration<P, D>,
): ToolRegistration<P, D> {
    return {
        ...tool,
        execute: async (args, api, context) => {
            const place = await placeOf(home, api, api.conversationId, context);
            const { path } = args as { path: string };
            return tool.execute(
                place === undefined
                    ? args
                    : { ...args, path: footing(path, place) },
                api,
                context,
            );
        },
    };
}
