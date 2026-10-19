// Provider-scoped, host-serialized credentials. Nothing here has a disk store.
import type { Credential, CredentialStore } from "@earendil-works/pi-ai";
import type { Channel } from "./channel.ts";
import type {
    CredentialRequest,
    Credential as WireCredential,
} from "./link.ts";

/** A link credential, retaining provider-specific OAuth fields. */
export function pack(credential: Credential): WireCredential {
    if (credential.type === "api_key") return credential;
    const { type, access, refresh, expires, ...extra } = credential;
    return { type, access, refresh, expires, extra };
}

/** Pi's canonical credential from its link envelope. */
export function unpack(credential: WireCredential): Credential {
    if (credential.type === "api_key") return credential;
    const { extra, ...known } = credential;
    return { ...extra, ...known };
}

/** Only this authorized attempt or provider operation can use this store. */
export function linkCredentials(
    channel: Channel,
    scope: string,
    provider: string,
    jobs: Set<Promise<unknown>>,
): CredentialStore {
    const request = async (
        fields: Omit<CredentialRequest, "v" | "id" | "type" | "scope">,
    ) => {
        const answer = await channel.request({
            type: "credential_request",
            scope,
            ...fields,
        });
        if (answer.type !== "credential_result")
            throw new Error("The host refused credential access.");
        if (answer.error !== undefined) throw new Error(answer.error);
        return answer;
    };
    const check = (id: string) => {
        if (id !== provider)
            throw new Error("Credential access is scoped to one provider.");
    };
    return {
        read: async (id, options) => {
            check(id);
            options?.signal?.throwIfAborted();
            const answer = await request({ action: "read" });
            options?.signal?.throwIfAborted();
            return answer.credential === undefined
                ? undefined
                : unpack(answer.credential);
        },
        list: async () => [],
        delete: async () => {
            throw new Error("This scope cannot delete credentials.");
        },
        modify: (id, change, options) => {
            const job = (async () => {
                check(id);
                options?.signal?.throwIfAborted();
                const held = await request({ action: "acquire" });
                if (held.lease === undefined)
                    throw new Error(
                        "The host did not grant a credential lease.",
                    );
                let committed = false;
                try {
                    // Cancellation can prevent a queued callback, never discard a rotation already started.
                    options?.signal?.throwIfAborted();
                    const next = await change(
                        held.credential === undefined
                            ? undefined
                            : unpack(held.credential),
                    );
                    // Commit releases the host lease even when persistence fails.
                    committed = true;
                    const saved = await request({
                        action: "commit",
                        lease: held.lease,
                        ...(next === undefined
                            ? {}
                            : { credential: pack(next) }),
                    });
                    return saved.credential === undefined
                        ? undefined
                        : unpack(saved.credential);
                } finally {
                    if (!committed)
                        await request({ action: "release", lease: held.lease });
                }
            })();
            jobs.add(job);
            return job.finally(() => jobs.delete(job));
        },
    };
}
