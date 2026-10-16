import type { ExecutionEnv } from "@earendil-works/pi-durable/env";
import type { Credentials, HomeBase } from "./home.ts";
import { shellOutput } from "./scrub/output.ts";
import { Scrubber } from "./scrub/scrubber.ts";

/** Credential scrubbing before storage or display, using the home's current credentials. */
export type Secrets = {
    words(text: string): string;
    /** Also removes transient sign-in secrets when supplied. */
    value<T>(value: T, extra?: readonly string[]): T;
    /** Scrubs streamed and spilled shell output before it leaves memory. */
    shell(env: ExecutionEnv): ExecutionEnv;
};

/** A home-owned capability that reads credentials afresh for each operation. */
export function secretsFrom(credentials: Credentials): Secrets {
    return {
        words: (text) => scrubValue(credentials.secrets(), text),
        value: (value, extra = []) =>
            scrubValue([...credentials.secrets(), ...extra], value),
        shell: (env) => shellOutput(env, credentials.secrets()),
    };
}

/** Words with every known credential removed before storage or display. */
export function safeWords(home: HomeBase, text: string): string {
    return home.secrets.words(text);
}

/** A JSON value with credentials removed from its strings and object keys. */
export function safeValue<T>(
    home: HomeBase,
    value: T,
    extra: readonly string[] = [],
): T {
    return home.secrets.value(value, extra);
}

function scrubValue<T>(secrets: readonly string[], value: T): T {
    const scrubber = new Scrubber(secrets);
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
    // SAFETY: scrubbing replaces only strings and keys in JSON values, preserving their declared shape; TypeScript cannot express this recursive transformation.
    // Verified by: scrub.test.ts structured output and envelope.test.ts receipts and refusals.
    return scrub(value) as T;
}

export { shellOutput } from "./scrub/output.ts";
export { REDACTED, Scrubber } from "./scrub/scrubber.ts";
