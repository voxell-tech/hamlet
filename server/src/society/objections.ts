import type { Context } from "@earendil-works/chord";
import type { HookApi } from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import type { Part } from "../parts.ts";
/** The hearings of agents' objections. */
export const Objections: Part = () => ({});
/** The hold before an instructed agent works. */
export async function holdFor(
    _home: Home,
    _api: HookApi,
    _context: Context,
): Promise<void> {
    return undefined;
}
