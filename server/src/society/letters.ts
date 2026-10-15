import type { Context } from "@earendil-works/chord";
import type { ToolExecutionApi } from "@earendil-works/pi-durable";
import type { AgentRef } from "../hamlet.ts";
import type { Home } from "../home.ts";
import type { Part } from "../parts.ts";
/** The letters and questions along roads. */
export const Letters: Part = () => ({});
/** A question to an agent, with its answer. */
export async function askAgent(
    _home: Home,
    _api: ToolExecutionApi,
    _to: AgentRef,
    _text: string,
    _context: Context,
): Promise<string> {
    throw new Error("This is not built yet.");
}
