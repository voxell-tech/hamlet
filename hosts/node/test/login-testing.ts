/** Helpers shared by the tests of the sign-ins. */
import assert from "node:assert/strict";
import type { TestContext } from "node:test";
import {
    type ApiKeyAuth,
    createFauxCore,
    createProvider,
    type FauxResponseFactory,
    fauxAssistantMessage,
    type Models,
    type MutableModels,
} from "@earendil-works/pi-ai";
import { chatActions } from "../src/agent/chat.ts";
import type { SignIns } from "../src/agent/login.ts";
import type { Login, LoginAnswer } from "../src/agent/messages/agent.ts";
import { from, type Woken } from "./agent-testing.ts";

/**
 * A made-up secret named for the test, so no test passes on a secret an
 * earlier one left in the host's memory. It holds a double quote, as A13's
 * key does, so a search must look for its escaped form too.
 */
export function secret(t: TestContext, what: string): string {
    return `made-up-${what}-"${t.name.replace(/\W+/g, "-")}`;
}

/**
 * Every form a secret takes in text: as it is, escaped in JSON, and escaped
 * twice, as in a row's JSON column dumped as JSON.
 */
export function forms(text: string): string[] {
    const once = JSON.stringify(text).slice(1, -1);
    return [text, once, JSON.stringify(once).slice(1, -1)];
}

export function assertNowhere(
    secrets: readonly string[],
    where: Record<string, unknown>,
): void {
    for (const [what, seen] of Object.entries(where)) {
        const text = typeof seen === "string" ? seen : JSON.stringify(seen);
        for (const found of secrets.flatMap(forms)) {
            assert.ok(!text.includes(found), `the ${what} hold ${found}`);
        }
    }
}

/**
 * The provider `loopback`, whose key sign-in asks as today's endpoint's
 * does (`harnesses/pi/src/providers.ts`, `compatible`), and whose one model
 * answers with `answer`.
 */
export function loopback(answer: FauxResponseFactory) {
    const core = createFauxCore({
        provider: "loopback",
        models: [{ id: "large", name: "Large", contextWindow: 64_000 }],
    });
    core.setResponses(Array.from({ length: 20 }, () => answer));
    const provider = createProvider({
        id: "loopback",
        models: core.models,
        api: { stream: core.stream, streamSimple: core.streamSimple },
        auth: {
            apiKey: {
                name: "loopback key",
                login: async (interaction) => ({
                    type: "api_key",
                    key: (
                        await interaction.prompt({
                            type: "secret",
                            message: "Paste your key.",
                        })
                    ).trim(),
                }),
                resolve: async ({ credential }) =>
                    credential?.key === undefined
                        ? undefined
                        : { auth: { apiKey: credential.key } },
            },
        },
    });
    return (listed: Models): Models => {
        (listed as MutableModels).setProvider(provider);
        return listed;
    };
}

let requests = 0;

/** The Hand's answers: it is never refused as not the Hand. */
type HandAnswer = Exclude<LoginAnswer, { readonly type: "not_the_hand" }>;

export function client(agent: Woken, given: SignIns) {
    const login = chatActions({ ...agent.config, signIns: given }).login;
    const answers: HandAnswer[] = [];
    let last: Login | undefined;
    const send = async (message: Login): Promise<HandAnswer> => {
        last = message;
        const answer = await login(agent.by(from.client), message);
        assert.notEqual(answer.type, "not_the_hand");
        answers.push(answer as HandAnswer);
        return answer as HandAnswer;
    };
    const call = (form: Omit<Login, "id">): Promise<HandAnswer> => {
        requests += 1;
        const id = `00000000-0000-4000-8000-${String(requests).padStart(12, "0")}`;
        return send({ id, ...form } as Login);
    };
    return {
        answers,
        start: (provider: string, method: "key" | "subscription") =>
            call({ start: { provider, method } }),
        reply: (login: string, value: string) =>
            call({ reply: { login, value } }),
        next: (login: string) => call({ reply: { login, value: "" } }),
        cancel: (login: string) => call({ cancel: login }),
        again: () => {
            if (last === undefined) throw new Error("nothing was called");
            return send(last);
        },
    };
}

export interface Fixture {
    readonly listing: (listed: Models) => Models;
    readonly code: string;
    readonly before: { readonly access: string; readonly refresh: string };
    readonly after: { readonly access: string; readonly refresh: string };
    begun: number;
    dropped: number;
    refreshes: number;
    readonly keysSeen: (string | undefined)[];
    readonly refreshHeld: PromiseWithResolvers<void>;
    beforePrompt: Promise<void>;
}

/**
 * The provider `chatgpt` on loopback, after today's OAuth fixture
 * (`harnesses/pi/test/fixtures/oauth.ts`): its sign-in shows a link, a
 * device code and a progress line, then asks for the code. Its model
 * echoes every token it knows, as A19's does.
 */
export function subscription(t: TestContext): Fixture {
    const fixture: Fixture = {
        listing: (listed) => listed,
        code: secret(t, "code"),
        before: {
            access: secret(t, "access-before"),
            refresh: secret(t, "refresh-before"),
        },
        after: {
            access: secret(t, "access-after"),
            refresh: secret(t, "refresh-after"),
        },
        begun: 0,
        dropped: 0,
        refreshes: 0,
        keysSeen: [],
        refreshHeld: Promise.withResolvers<void>(),
        beforePrompt: Promise.resolve(),
    };
    const core = createFauxCore({
        provider: "chatgpt",
        models: [{ id: "offline", name: "Offline", contextWindow: 64_000 }],
    });
    const echo: FauxResponseFactory = (_transcript, options) => {
        fixture.keysSeen.push(options?.apiKey);
        const { before, after } = fixture;
        return fauxAssistantMessage(
            `OAuth echoed ${before.access} ${before.refresh} ${after.access} ${after.refresh}`,
        );
    };
    core.setResponses(Array.from({ length: 20 }, () => echo));
    const provider = createProvider({
        id: "chatgpt",
        models: core.models,
        api: { stream: core.stream, streamSimple: core.streamSimple },
        auth: {
            oauth: {
                name: "loopback subscription",
                login: async (interaction, options) => {
                    fixture.begun += 1;
                    interaction.signal.addEventListener("abort", () => {
                        fixture.dropped += 1;
                    });
                    interaction.notify({
                        type: "device_code",
                        verificationUri: "https://example.invalid/oauth",
                        userCode: "SAFE-CODE",
                    });
                    interaction.notify({
                        type: "progress",
                        message: "Waiting for the offline code",
                    });
                    await fixture.beforePrompt;
                    const code = await interaction.prompt({
                        type: "manual_code",
                        message: "Paste the code.",
                    });
                    if (code !== fixture.code) {
                        throw new Error(`The code ${code} was rejected.`);
                    }
                    return {
                        type: "oauth",
                        ...fixture.before,
                        expires: Date.now() + 3_600_000,
                        clientId: options?.getDeviceId?.(),
                        agent: options?.agentName,
                        scopes: ["offline", "profile"],
                    };
                },
                refresh: async (credential) => {
                    fixture.refreshes += 1;
                    await fixture.refreshHeld.promise;
                    return {
                        ...credential,
                        ...fixture.after,
                        expires: Date.now() + 3_600_000,
                    };
                },
                toAuth: async (credential) => ({ apiKey: credential.access }),
            },
        },
    });
    return Object.assign(fixture, {
        listing: (listed: Models): Models => {
            (listed as MutableModels).setProvider(provider);
            return listed;
        },
    });
}

export function keySignIn(
    id: string,
    login: NonNullable<ApiKeyAuth["login"]>,
): (listed: Models) => Models {
    const unused = () => {
        throw new Error("unused");
    };
    const provider = createProvider({
        id,
        models: [],
        api: { stream: unused, streamSimple: unused },
        auth: { apiKey: { name: id, login, resolve: async () => undefined } },
    });
    return (listed) => {
        (listed as MutableModels).setProvider(provider);
        return listed;
    };
}
