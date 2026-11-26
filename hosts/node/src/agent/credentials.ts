/**
 * The credentials the realm gives this host, held in memory only, and the
 * host's scrub list of every secret it was given.
 */
import type {
    Credential,
    Models,
    OAuthAuth,
    OAuthCredential,
    ProviderEnv,
    ProviderHeaders,
} from "@earendil-works/pi-ai";
import { PROVIDER as CLAUDE } from "./claude.ts";
import type { CredentialAnswer, Kept, OauthLogin } from "./messages/realm.ts";
import { u32 } from "./messages/shared.ts";
import { isEndpoint } from "./providers.ts";
import { type BareRealm, type Peers, reasonOf } from "./turn.ts";

/** The provider of the faux models, which take no credential. */
export const FAUX = "faux";

/**
 * Every key, token and typed sign-in answer this host was given, and every
 * value of the realm's secret list, retired ones included. They live only here,
 * for the host's life.
 */
const SECRETS = new Set<string>();

export function remember(...secrets: readonly string[]): void {
    for (const secret of secrets) if (secret !== "") SECRETS.add(secret);
}

/** The host's first load of the realm's secret list. */
let listLoaded: Promise<void> | undefined;

/**
 * The realm's whole secret list taken into the scrub, once in the host's
 * life; every read a client or a model makes waits for it. A load that
 * fails is tried again by the next read, so nothing leaves unscrubbed.
 */
export function secretsLoaded(
    realm: Pick<BareRealm, "secrets">,
): Promise<void> {
    listLoaded ??= realm.secrets().then(
        ({ values }) => remember(...values),
        (error: unknown) => {
            listLoaded = undefined;
            throw error;
        },
    );
    return listLoaded;
}

/** Takes the realm's secret list again after its first load. */
export async function secretsRefreshed(
    realm: Pick<BareRealm, "secrets">,
): Promise<void> {
    const first = listLoaded;
    await secretsLoaded(realm);
    if (first !== undefined) remember(...(await realm.secrets()).values);
}

/** Remembers the secrets of `kept`. */
export function rememberKept(kept: Kept): void {
    if (kept.type === "key") remember(kept.key);
    else remember(kept.login.access, kept.login.refresh);
}

/**
 * `value` with every secret the host holds, and each of `also`, replaced from
 * the left: at each place the longest secret that begins there.
 */
export function scrub<T>(value: T, also: readonly string[] = []): T {
    const secrets = [...SECRETS, ...also.filter((word) => word !== "")].sort(
        (a, b) => b.length - a.length,
    );
    if (secrets.length === 0) return value;
    const clean = (text: string) => {
        let out = "";
        let at = 0;
        while (at < text.length) {
            const secret = secrets.find((word) => text.startsWith(word, at));
            if (secret === undefined) {
                out += text[at];
                at += 1;
            } else {
                out += "[REDACTED]";
                at += secret.length;
            }
        }
        return out;
    };
    return JSON.parse(
        JSON.stringify(value, (_key, item: unknown) => {
            if (typeof item === "string") return clean(item);
            if (
                typeof item === "object" &&
                item !== null &&
                !Array.isArray(item)
            ) {
                return Object.fromEntries(
                    Object.entries(item).map(([key, inner]) => [
                        clean(key),
                        inner,
                    ]),
                );
            }
            return item;
        }),
    ) as T;
}

/** `text` without a tail that begins a secret the host holds. */
export function withheld(text: string): string {
    let cut = 0;
    for (const secret of SECRETS) {
        for (let length = secret.length - 1; length > cut; length -= 1) {
            if (text.endsWith(secret.slice(0, length))) cut = length;
        }
    }
    return text.slice(0, text.length - cut);
}

/** The credential the realm gave this wake, per provider. */
export interface Keys {
    /**
     * The key and the settings a request to `provider` carries, neither for
     * the faux models and for `claude`. Without either it throws, so no key
     * from the host's environment is used.
     */
    of(provider: string): Promise<Given>;
    /**
     * Rotates the realm's OAuth login for `provider` when it is due, as a
     * request would; nothing for a key or where the realm holds none.
     */
    fresh(provider: string): Promise<void>;
}

/** The part of its credential a request carries, as pi-ai's request options. */
export interface Given {
    readonly apiKey?: string;
    readonly env?: ProviderEnv;
    /** The headers a login's auth adds, as pi-ai's `toAuth` gives them. */
    readonly headers?: ProviderHeaders;
}

/**
 * The parts an OAuth sign-in needs to serve requests: the listing's sign-in
 * for its provider, and the realm's keep for a rotated login. Without it,
 * only keys are used.
 */
export interface Subscriptions {
    readonly listed: Pick<Models, "getProvider">;
    readonly keep: BareRealm["keep_credential"];
}

/**
 * As pi-ai's `Models.getAuth` (`auth/resolve.js`): a login with less left
 * than this is refreshed before a request.
 */
const REFRESH_WITHIN_MS = 5 * 60_000;

export function keysOf(
    realm: Pick<BareRealm, "credential">,
    report: Peers["report"],
    subscriptions?: Subscriptions,
): Keys {
    const asked = new Map<string, Promise<CredentialAnswer>>();
    const ask = async (provider: string): Promise<CredentialAnswer> => {
        let answer: CredentialAnswer;
        try {
            answer = await realm.credential({ provider });
        } catch (error) {
            const words = `the realm's credential for ${provider} was not read`;
            report(words, error);
            throw new Error(`${words}: ${reasonOf(error)}`);
        }
        if (answer.type !== "refused") rememberKept(answer);
        return answer;
    };
    const givenOf = async (provider: string, kept: Kept): Promise<Given> => {
        if (
            kept.type === "key" &&
            (kept.key !== "" || kept.env !== undefined)
        ) {
            // An empty key would stand over the one pi-ai resolves from the settings.
            return {
                ...(kept.key === "" ? {} : { apiKey: kept.key }),
                ...(kept.env === undefined ? {} : { env: kept.env }),
            };
        }
        const oauth = subscriptions?.listed.getProvider(provider)?.auth.oauth;
        if (
            kept.type === "oauth" &&
            oauth !== undefined &&
            subscriptions !== undefined
        ) {
            let login = kept.login;
            if (login.expires * 1000 <= Date.now() + REFRESH_WITHIN_MS) {
                login = await rotated(provider, login, oauth, {
                    keep: subscriptions.keep,
                    report,
                });
                asked.set(provider, Promise.resolve({ type: "oauth", login }));
            }
            const { apiKey, headers } = await oauth.toAuth(credentialOf(login));
            if (apiKey !== undefined)
                return {
                    apiKey,
                    ...(headers === undefined ? {} : { headers }),
                };
        }
        throw new Error(
            `The sign-in for ${provider} gives no key, and only a key is used yet.`,
        );
    };
    const keptFor = (provider: string): Promise<CredentialAnswer> => {
        let asking = asked.get(provider);
        if (asking === undefined) {
            asking = ask(provider);
            asked.set(provider, asking);
            const forget = () => asked.delete(provider);
            asking.then((answer) => {
                if (answer.type === "refused") forget();
            }, forget);
        }
        return asking;
    };
    return {
        fresh: async (provider) => {
            if (provider === FAUX) return;
            const kept = await keptFor(provider);
            if (kept.type === "oauth") await givenOf(provider, kept);
        },
        of: async (provider) => {
            if (provider === FAUX) return {};
            const kept = await keptFor(provider);
            if (kept.type === "refused") {
                const listed = subscriptions?.listed.getProvider(provider);
                if (listed !== undefined && isEndpoint(listed)) return {};
                throw new Error(kept.words);
            }
            // The Claude program signs in itself; the realm's credential only marks that it did.
            if (provider === CLAUDE) return {};
            return givenOf(provider, kept);
        },
    };
}

/**
 * Each rotation by the login it retires, so one refresh runs per login in
 * the host and every agent still holding the old one takes the new.
 */
const ROTATIONS = new Map<string, Promise<OauthLogin>>();

/** A rotation's signal: one begun runs to its end, whoever asked for it. */
const NEVER_ABORTED = new AbortController().signal;

/**
 * `login` refreshed, once in the host, and handed to the realm before it is
 * used.
 */
function rotated(
    provider: string,
    login: OauthLogin,
    oauth: OAuthAuth,
    to: { keep: BareRealm["keep_credential"]; report: Peers["report"] },
): Promise<OauthLogin> {
    const retired = `${provider}\n${login.refresh}`;
    let rotation = ROTATIONS.get(retired);
    if (rotation === undefined) {
        rotation = (async () => {
            const fresh = loginOf(
                await oauth.refresh(credentialOf(login), NEVER_ABORTED),
            );
            const value: Kept = { type: "oauth", login: fresh };
            rememberKept(value);
            try {
                await to.keep({ provider, value });
            } catch (error) {
                to.report(
                    `the rotated login for ${provider} was not kept`,
                    scrubbedError(error),
                );
            }
            return fresh;
        })();
        ROTATIONS.set(retired, rotation);
        rotation.catch(() => ROTATIONS.delete(retired));
    }
    return rotation;
}

/** Resolves once each rotation begun in the host has been handed to the realm. */
export async function rotationsKept(): Promise<void> {
    await Promise.allSettled(ROTATIONS.values());
}

/** pi-ai's credential as the realm keeps it: an OAuth expiry in whole seconds. */
export function keptOf(credential: Credential): Kept {
    if (credential.type === "api_key") {
        const { key = "", env = {} } = credential;
        return {
            type: "key",
            key,
            ...(Object.keys(env).length === 0 ? {} : { env }),
        };
    }
    return { type: "oauth", login: loginOf(credential) };
}

function loginOf(credential: OAuthCredential): OauthLogin {
    const { type: _, access, refresh, expires, ...extra } = credential;
    return {
        access,
        refresh,
        expires: u32(Math.floor(expires / 1000)),
        ...(Object.keys(extra).length === 0
            ? {}
            : { extra: JSON.stringify(extra) }),
    };
}

function credentialOf(login: OauthLogin): OAuthCredential {
    return {
        ...(login.extra === undefined
            ? {}
            : (JSON.parse(login.extra) as Record<string, unknown>)),
        type: "oauth",
        access: login.access,
        refresh: login.refresh,
        expires: login.expires * 1000,
    };
}

/** The spend gate keeps a thrown error's words as the answer. */
export function scrubbedError(error: unknown): Error {
    return new Error(scrub(reasonOf(error)));
}
