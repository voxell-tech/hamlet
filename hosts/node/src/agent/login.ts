/**
 * Sign-in to a provider, run in the Node host: each call answers the next
 * step, and the end hands the credential to the realm. What a sign-in holds
 * is in memory only.
 */
import type { Kept, Login, LoginAnswer } from "./messages.ts";

export interface SignIns {
    step(login: Login): Promise<LoginAnswer>;
}

export function signIns(
    keep: (provider: string, value: Kept) => Promise<void>,
): SignIns {
    void keep;
    return {
        step: () => Promise.reject(new Error("not built: U8")),
    };
}
