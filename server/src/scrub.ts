import type { HomeBase } from "./home.ts";
import { Scrubber } from "./scrub/scrubber.ts";

/** Words with every known credential removed before storage or display. */
export function safeWords(home: HomeBase, text: string): string {
    return safeValue(home, text);
}

/** A JSON value with credentials removed from its strings and object keys. */
export function safeValue<T>(
    home: HomeBase,
    value: T,
    extra: readonly string[] = [],
): T {
    const scrubber = new Scrubber([...home.credentials.secrets(), ...extra]);
    const scrub = (value: unknown): unknown => {
        if (typeof value === "string")
            return scrubber.push(value) + scrubber.flush();
        if (Array.isArray(value)) return value.map(scrub);
        if (value !== null && typeof value === "object")
            return Object.fromEntries(
                Object.entries(value).map(([key, value]) => [
                    scrubber.push(key) + scrubber.flush(),
                    scrub(value),
                ]),
            );
        return value;
    };
    return scrub(value) as T;
}
export { shellOutput } from "./scrub/output.ts";
export { REDACTED, Scrubber } from "./scrub/scrubber.ts";
