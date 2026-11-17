/**
 * Sign-in to a provider, run in the Node host: each call answers the next
 * step, and the end hands the credential to the realm. What a sign-in holds
 * is in memory only, so a kill drops it and the client starts again.
 */
import { setTimeout as sleep } from "node:timers/promises";
import type {
    AuthEvent,
    AuthPrompt,
    Credential,
    Models,
    ProviderAuthInteraction,
} from "@earendil-works/pi-ai";
import type {
    Kept,
    Login,
    LoginAnswer,
    LoginStep,
    RequestId,
} from "./messages.ts";
import { keptOf, remember, rememberKept, scrub } from "./models.ts";
import type { BareRealm } from "./turn.ts";

/** How long a call waits for its step, as the front door allows it (root point 1). */
const STEP_WAIT_MS = 25_000;

/**
 * The providers a chief signs in to with a subscription (ADR 0006), as
 * `harnesses/pi/src/provider-set.ts` names them.
 */
const SUBSCRIPTIONS = ["chatgpt", "openrouter", "claude"];

/** As `crates/hamlet_server/src/providers.rs` words them. */
const NOT_OPEN = "No sign-in of yours is open.";
const CANCELLED = "Sign-in cancelled.";

/** The realm's keep, reached from the call that ends a sign-in. */
export type Keep = BareRealm["keep_credential"];

export interface SignIns {
    step(login: Login, keep: Keep): Promise<LoginAnswer>;
}

export interface SignInOptions {
    /** The installation's id, which a provider's sign-in may name the host by. */
    readonly deviceId?: string;
    /**
     * Resolves when a call has waited long enough for a step. Tests hold it.
     * `stop` aborts once the call has its answer.
     */
    readonly waited?: (stop: AbortSignal) => Promise<void>;
}

type Flow = (interaction: ProviderAuthInteraction) => Promise<Credential>;

/**
 * The sign-ins this host ran, by login. One per provider is open: a new
 * start drops the one before, which a client that went away left.
 */
export function signIns(
    listed: Pick<Models, "getProvider">,
    options: SignInOptions = {},
): SignIns {
    const all = new Map<string, SignIn>();
    const waited =
        options.waited ??
        ((stop) =>
            sleep(STEP_WAIT_MS, undefined, { ref: false, signal: stop }).catch(
                () => {},
            ));
    const flowOf = (
        provider: string,
        method: "key" | "subscription",
    ): Flow | string => {
        const auth = listed.getProvider(provider)?.auth;
        if (auth === undefined) return `Unknown provider: ${provider}`;
        const { apiKey, oauth } = auth;
        const keyLogin = apiKey?.login;
        if (method === "key" && keyLogin !== undefined) {
            return (interaction) => keyLogin.call(apiKey, interaction);
        }
        if (
            method === "subscription" &&
            oauth !== undefined &&
            SUBSCRIPTIONS.includes(provider)
        ) {
            const { deviceId } = options;
            // Hamlet signs in as itself (S8), as today's harness does.
            return (interaction) =>
                oauth.login(interaction, {
                    agentName: "Hamlet",
                    ...(deviceId === undefined
                        ? {}
                        : { getDeviceId: () => deviceId }),
                });
        }
        return `Signing in with ${provider} is not offered. Use an API key.`;
    };
    return {
        step: async (login, keep) => {
            const at =
                "start" in login
                    ? login.id
                    : "reply" in login
                      ? login.reply.login
                      : login.cancel;
            let signIn = all.get(at);
            const repeat = signIn?.repeat(login.id);
            if (repeat !== undefined) return repeat;
            if ("start" in login) {
                const { provider, method } = login.start;
                const flow = flowOf(provider, method);
                if (typeof flow === "string") return failed(at, flow);
                for (const open of all.values()) {
                    if (open.provider === provider && !open.closed)
                        open.cancel();
                }
                signIn = new SignIn(provider, waited, flow);
                all.set(at, signIn);
            }
            if (signIn === undefined || signIn.closed) {
                return failed(at, NOT_OPEN);
            }
            const open = signIn;
            if ("cancel" in login) {
                open.cancel();
                return open.answer(login.id, async () => failed(at, CANCELLED));
            }
            if ("reply" in login) open.reply(login.reply.value);
            return open.answer(login.id, () => open.next(at, keep));
        },
    };
}

function failed(login: string, words: string): LoginAnswer {
    return { type: "failed", login, words };
}

/** How a flow ended: its credential as the realm keeps it, or the words it failed with. */
type End = { readonly kept: Kept } | { readonly failed: string };

/** One sign-in: the steps its flow gave that no call has answered yet. */
class SignIn {
    readonly provider: string;
    /** Answered with its end, after which only a repeat is answered. */
    closed = false;
    readonly #waited: (stop: AbortSignal) => Promise<void>;
    readonly #stop = new AbortController();
    readonly #steps: LoginStep[] = [];
    /** The step the last call answered with. */
    #given: LoginStep | undefined;
    #asking:
        | {
              readonly step: LoginStep;
              readonly secret: boolean;
              readonly answer: (value: string) => void;
          }
        | undefined;
    /** What the chief typed, scrubbed from this sign-in's failure. */
    readonly #answers: string[] = [];
    #end: End | undefined;
    #changed = Promise.withResolvers<void>();
    #last:
        | { readonly id: RequestId; readonly answer: Promise<LoginAnswer> }
        | undefined;

    constructor(
        provider: string,
        waited: (stop: AbortSignal) => Promise<void>,
        flow: Flow,
    ) {
        this.provider = provider;
        this.#waited = waited;
        flow({
            signal: this.#stop.signal,
            prompt: (prompt) => this.#prompt(prompt),
            notify: (event) => {
                for (const step of stepsOf(event)) this.#push(step);
            },
        }).then(
            (credential) => this.#ended({ kept: keptOf(credential) }),
            (error: unknown) => this.#ended({ failed: String(error) }),
        );
    }

    /** The answer a call with `id` was given, when it was the last call. */
    repeat(id: RequestId): Promise<LoginAnswer> | undefined {
        return this.#last?.id === id ? this.#last.answer : undefined;
    }

    answer(
        id: RequestId,
        make: () => Promise<LoginAnswer>,
    ): Promise<LoginAnswer> {
        const answer = make();
        this.#last = { id, answer };
        return answer;
    }

    /**
     * Gives `value` to the prompt the last call answered with. A reply to
     * any other step asks only for the next one.
     */
    reply(value: string): void {
        const asking = this.#asking;
        if (asking === undefined || asking.step !== this.#given) return;
        this.#asking = undefined;
        this.#answers.push(value);
        // Only a secret: a choice or a name such as `default` would be
        // scrubbed from everything the host shows.
        if (asking.secret) remember(value);
        asking.answer(value);
    }

    cancel(): void {
        this.closed = true;
        this.#end ??= { failed: CANCELLED };
        this.#steps.length = 0;
        this.#stop.abort();
        this.#wake();
    }

    /**
     * The next step the flow gave, or its end with the credential kept.
     * With nothing new in time, the step given last again.
     */
    async next(login: string, keep: Keep): Promise<LoginAnswer> {
        const stop = new AbortController();
        const late = this.#waited(stop.signal).then(() => true);
        try {
            for (;;) {
                const step = this.#steps.shift();
                if (step !== undefined) {
                    this.#given = step;
                    return { type: "step", login, step };
                }
                if (this.#end !== undefined) {
                    return await this.#close(login, this.#end, keep);
                }
                const changed = this.#changed.promise.then(() => false);
                if (await Promise.race([changed, late])) break;
            }
        } finally {
            stop.abort();
        }
        if (this.#given === undefined) {
            throw new Error("the sign-in gave no step in time");
        }
        return { type: "step", login, step: this.#given };
    }

    async #close(login: string, end: End, keep: Keep): Promise<LoginAnswer> {
        this.closed = true;
        if ("failed" in end) {
            return failed(login, scrub(end.failed, this.#answers));
        }
        try {
            await keep({ provider: this.provider, value: end.kept });
        } catch (error) {
            return failed(login, scrub(String(error)));
        }
        return { type: "done", login };
    }

    #prompt(prompt: AuthPrompt): Promise<string> {
        return new Promise<string>((resolve, reject) => {
            const signal =
                prompt.signal === undefined
                    ? this.#stop.signal
                    : AbortSignal.any([this.#stop.signal, prompt.signal]);
            if (signal.aborted) {
                reject(new Error(CANCELLED));
                return;
            }
            // As `harnesses/pi/src/sign-in.ts` shows a prompt.
            const choices =
                prompt.type === "select"
                    ? `\n${prompt.options.map((o) => `${o.id}: ${o.label}`).join("\n")}`
                    : "";
            const secret =
                prompt.type === "secret" || prompt.type === "manual_code";
            const step: LoginStep = {
                prompt: prompt.message + choices,
                secret,
            };
            this.#asking = { step, secret, answer: resolve };
            signal.addEventListener(
                "abort",
                () => {
                    // The flow went on without it, as when a browser's
                    // callback wins.
                    if (this.#asking?.step === step) this.#asking = undefined;
                    const queued = this.#steps.indexOf(step);
                    if (queued !== -1) this.#steps.splice(queued, 1);
                    reject(new Error(CANCELLED));
                },
                { once: true },
            );
            this.#push(step);
        });
    }

    #push(step: LoginStep): void {
        this.#steps.push(step);
        this.#wake();
    }

    #ended(end: End): void {
        if (this.#end !== undefined) return;
        if ("kept" in end) rememberKept(end.kept);
        this.#end = end;
        this.#wake();
    }

    #wake(): void {
        this.#changed.resolve();
        this.#changed = Promise.withResolvers<void>();
    }
}

/** As `harnesses/pi/src/sign-in.ts` turns pi-ai's events into steps. */
function stepsOf(event: AuthEvent): LoginStep[] {
    switch (event.type) {
        case "auth_url":
            return [
                {
                    url: event.url,
                    ...(event.instructions === undefined
                        ? {}
                        : { instructions: event.instructions }),
                },
            ];
        case "device_code":
            return [{ url: event.verificationUri }, { code: event.userCode }];
        case "info":
            return [
                ...(event.links ?? []).map((link) => ({ url: link.url })),
                { progress: event.message },
            ];
        case "progress":
            return [{ progress: event.message }];
    }
}
