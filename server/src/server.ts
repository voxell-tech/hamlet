import { randomUUID } from "node:crypto";
import type { Context } from "@earendil-works/chord";
import { withAbortSignal } from "@earendil-works/chord/context";
import type { AuthPrompt } from "@earendil-works/pi-ai";
import type { Submission } from "@earendil-works/pi-durable";
import {
    type ChatId,
    newChat,
    resumeChat,
    retry,
    rewind,
    stopAgent,
    stopChiefAgents,
} from "./chats.ts";
import {
    type Client,
    connectionFailureText,
    type Server,
    serve,
} from "./connection.ts";
import { answerWords } from "./gate.ts";
import type { Home } from "./home.ts";
import { clearStagedGifts, giftRefusalText, give, uploads } from "./house.ts";
import { loginFailureText } from "./models.ts";
import type { Address, Login, LoginStep, ServerMessage } from "./protocol.ts";
import { chiefAccess, legacyRefusal } from "./realm/access.ts";
import { resolve } from "./realm/roads.ts";
import { err, ok, type Result } from "./result.ts";
import { say } from "./say.ts";
import { edit } from "./server/edits.ts";
import {
    type ChiefFailure,
    faultFor,
    type Receipt,
    replyFor,
} from "./server/envelope.ts";
import { read } from "./server/reads.ts";
import { watchState } from "./state.ts";
import {
    type ChatWatch,
    pageChat,
    requireWritable,
    turnTools,
    watchChat,
} from "./transcript.ts";

/** Serves `home` over protocol v0 on 127.0.0.1 at `port`, to clients with its token (S4, S5). */
export async function serveHome(
    home: Home,
    port: number,
    context: Context,
): Promise<Server> {
    home.registry.install(turnTools());
    let server: Server | undefined;
    const state = await watchState(
        home,
        (message) => server?.broadcast(home.secrets.value(message)),
        context,
    );
    // Each client's open chats, by the chat as it named them.
    const watching = new Map<Client, Map<string, ChatWatch>>();
    const stop = async (client: Client, chat: string) => {
        const watches = watching.get(client);
        await watches?.get(chat)?.stop();
        watches?.delete(chat);
    };
    const changed = async (
        outcome: string | Result<string, ChiefFailure>,
    ): Promise<Result<Receipt, ChiefFailure>> => {
        if (typeof outcome !== "string" && !outcome.ok) return outcome;
        await state.refresh();
        return ok({
            text: typeof outcome === "string" ? outcome : outcome.value,
        });
    };
    const chatOf = async (
        address: Address,
    ): Promise<Result<ChatId, ChiefFailure>> => {
        const access = await chiefAccess(
            home,
            address.hamlet,
            "change chat",
            context,
        );
        if (!access.ok) return access;
        const target = await resolve(
            home.harness,
            address.hamlet,
            `${address.agent}@${address.hamlet}`,
            context,
        );
        if (!target)
            return err({ kind: "agent-missing", agent: address.agent });
        if ("gone" in target)
            return err({ kind: "agent-gone", farewell: target.gone });
        return ok({ agent: target.ref, chief: home.chief });
    };
    const cleared = await clearStagedGifts(home);
    if (!cleared.ok) throw new Error(giftRefusalText(cleared.error));
    const gifts = uploads(home, context);
    const logins = loginConnection(home, () => state.refresh());
    server = await serve({
        token: home.credentials.token,
        port,
        fault: (to, failure) => faultFor(to, failure, home.secrets),
        report: (error, to) =>
            console.error(
                home.secrets.words(
                    `Request ${to ?? "(connection)"} failed: ${connectionFailureText({ kind: "defect", cause: error })}`,
                ),
            ),
        handlers: {
            hello: async () => home.secrets.value(await state.snapshot()),
            say: async (message, client) => {
                let work: Submission | undefined;
                const outcome = await say(
                    home,
                    message,
                    context,
                    (submission) => {
                        work = submission;
                    },
                );
                await state.refresh();
                if (outcome !== undefined)
                    return replyFor(message.id, outcome, home.secrets);
                const done = replyFor(
                    message.id,
                    ok({ text: "" }),
                    home.secrets,
                );
                if (work === undefined) return done;
                // This receipt settles the client's line, not the whole conversation.
                void work
                    .wait(withAbortSignal(client.signal, context))
                    .then(() => {
                        if (!client.signal.aborted) client.send(done);
                    })
                    .catch((error: unknown) => {
                        if (!client.signal.aborted)
                            client.fail(String(error), message.id);
                    });
                return undefined;
            },
            open: async (message, client) => {
                const chat = JSON.stringify(message.chat);
                if (
                    message.before !== undefined &&
                    watching.get(client)?.has(chat)
                ) {
                    const page = await pageChat(
                        home,
                        message.chat,
                        message.before,
                        context,
                    );
                    if (!page.ok)
                        return faultFor(message.id, page.error, home.secrets);
                    return home.secrets.value({
                        ...page.value,
                        to: message.id,
                    });
                }
                const previous = watching.get(client)?.get(chat);
                let buffered: ServerMessage[] | undefined = [];
                const outcome = await watchChat(
                    home,
                    message.chat,
                    (outcome, to) => {
                        const message = outcome.ok
                            ? home.secrets.value(outcome.value)
                            : faultFor(to, outcome.error, home.secrets);
                        if (buffered !== undefined) buffered.push(message);
                        else if (!client.signal.aborted) client.send(message);
                    },
                    message.id,
                    context,
                    message.before,
                );
                if (!outcome.ok)
                    return faultFor(message.id, outcome.error, home.secrets);
                const watch = outcome.value;
                if (client.signal.aborted) {
                    await watch.stop();
                    return undefined;
                }
                if (!watching.has(client)) {
                    watching.set(client, new Map());
                    client.signal.addEventListener("abort", () => {
                        const watches = watching.get(client);
                        watching.delete(client);
                        for (const each of watches?.values() ?? [])
                            void each.stop();
                    });
                }
                watching.get(client)?.set(chat, watch);
                await previous?.stop();
                const initial = buffered;
                buffered = undefined;
                if (!client.signal.aborted)
                    for (const message of initial) client.send(message);
                return undefined;
            },
            login: async (message, client) => {
                const outcome = await logins.handle(message, client);
                return outcome === undefined
                    ? undefined
                    : replyFor(message.id, outcome, home.secrets);
            },
            check: (message, client) => {
                void (async () => {
                    const outcome = await changed(
                        message.what === "connection"
                            ? await home.modelHome.checkConnection(context)
                            : await home.modelHome.checkModel(
                                  message.model,
                                  context,
                              ),
                    );
                    if (!client.signal.aborted)
                        client.send(
                            replyFor(message.id, outcome, home.secrets),
                        );
                })().catch((error: unknown) => {
                    if (!client.signal.aborted) client.fail(error, message.id);
                });
                return undefined;
            },
            give: async (message, client) => {
                if (!("files" in message))
                    return replyFor(
                        message.id,
                        await changed(await give(home, message, context)),
                        home.secrets,
                    );
                return replyFor(
                    message.id,
                    await gifts.begin(message, client.signal),
                    home.secrets,
                );
            },

            "give-part": async (message, client) =>
                replyFor(
                    message.id,
                    await changed(await gifts.part(message, client.signal)),
                    home.secrets,
                ),
            edit: async (message) =>
                replyFor(
                    message.id,
                    await changed(await edit(home, message, context)),
                    home.secrets,
                ),
            read: async (message) => {
                // D2/D3 replace this read-call compatibility catch with address and watch failures.
                const outcome = await read(home, message, context).catch(
                    legacyRefusal,
                );
                return outcome.ok
                    ? home.secrets.value(outcome.value)
                    : replyFor(message.id, outcome, home.secrets);
            },
            // D6 replaces this answer-call compatibility catch with answer-not-offered.
            answer: async (message) =>
                replyFor(
                    message.id,
                    await changed(
                        await answerWords(
                            home,
                            { ...message, chief: home.chief },
                            context,
                        ).catch(legacyRefusal),
                    ),
                    home.secrets,
                ),
            stop: async (message) => {
                // D3 replaces these stop-call compatibility catches with ConversationFailure.
                if (message.agent === undefined)
                    return replyFor(
                        message.id,
                        await changed(
                            await stopChiefAgents(home, context).catch(
                                legacyRefusal,
                            ),
                        ),
                        home.secrets,
                    );
                const chat = await chatOf(message.agent);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                return replyFor(
                    message.id,
                    await changed(
                        await stopAgent(home, chat.value.agent, context).catch(
                            legacyRefusal,
                        ),
                    ),
                    home.secrets,
                );
            },
            new: async (message) => {
                const chat = await chatOf(message.to);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                // D3 replaces this new-session compatibility catch with ConversationFailure.
                const outcome = await newChat(home, chat.value, context)
                    .then(() => ok("Started a new session."))
                    .catch(legacyRefusal);
                return replyFor(
                    message.id,
                    await changed(outcome),
                    home.secrets,
                );
            },
            resume: async (message) => {
                const chat = await chatOf(message.to);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                // D3 replaces this resume-call compatibility catch with ConversationFailure.
                const outcome = await resumeChat(
                    home,
                    chat.value,
                    message.number,
                    context,
                )
                    .then(() => ok("Went back to an earlier session."))
                    .catch(legacyRefusal);
                return replyFor(
                    message.id,
                    await changed(outcome),
                    home.secrets,
                );
            },
            retry: async (message) => {
                const chat = await chatOf(message.to);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                // D3 replaces this retry-call compatibility catch with ConversationFailure.
                const outcome = await retry(home, chat.value, context)
                    .then(() => ok("Carrying on from where it stopped."))
                    .catch(legacyRefusal);
                return replyFor(
                    message.id,
                    await changed(outcome),
                    home.secrets,
                );
            },
            rewind: async (message) => {
                const access = await chiefAccess(
                    home,
                    message.chat.with.hamlet,
                    "change chat",
                    context,
                );
                if (!access.ok)
                    return replyFor(message.id, access, home.secrets);
                // D3 replaces this writable-chat compatibility catch with WatchFailure.
                try {
                    requireWritable(message.chat);
                } catch (cause) {
                    return replyFor(
                        message.id,
                        legacyRefusal(cause),
                        home.secrets,
                    );
                }
                if (message.chat.past !== undefined)
                    return replyFor(
                        message.id,
                        legacyRefusal(
                            "Open the current conversation to rewind it.",
                        ),
                        home.secrets,
                    );
                const chat = await chatOf(message.chat.with);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                // D3 replaces this rewind-call compatibility catch with ConversationFailure.
                const outcome = await changed(
                    await rewind(home, chat.value, message.item, context).catch(
                        legacyRefusal,
                    ),
                );
                return replyFor(
                    message.id,
                    outcome.ok
                        ? ok({ ...outcome.value, kind: "rewound" })
                        : outcome,
                    home.secrets,
                );
            },
            close: async (message, client) => {
                await stop(client, JSON.stringify(message.chat));
                return undefined;
            },
        },
    });
    const serving = server;
    return {
        ...serving,
        close: async () => {
            await serving.close();
            await logins.stop();
            await gifts.stop();
            await state.stop();
        },
    };
}

function loginConnection(home: Home, refresh: () => Promise<void>) {
    type Signing = {
        client: Client;
        controller: AbortController;
        secrets: string[];
        prompt?: { resolve(value: string): void; reject(error: Error): void };
        job?: Promise<void>;
    };
    const sessions = new Map<string, Signing>();
    const handle = async (
        message: Login,
        client: Client,
    ): Promise<Result<Receipt, ChiefFailure> | undefined> => {
        if ("start" in message) {
            const id = randomUUID();
            const signing: Signing = {
                client,
                controller: new AbortController(),
                secrets: [],
            };
            sessions.set(id, signing);
            const signal = AbortSignal.any([
                signing.controller.signal,
                client.signal,
            ]);
            const send = (step: LoginStep) => {
                if (!client.signal.aborted)
                    client.send(
                        home.secrets.value(
                            { type: "login", login: id, to: message.id, step },
                            signing.secrets,
                        ),
                    );
            };
            const prompt = (prompt: AuthPrompt) =>
                new Promise<string>((resolve, reject) => {
                    const aborted = () =>
                        finish(undefined, new Error("Sign-in cancelled."));
                    const stop =
                        prompt.signal === undefined
                            ? signal
                            : AbortSignal.any([signal, prompt.signal]);
                    const finish = (value?: string, error?: Error) => {
                        stop.removeEventListener("abort", aborted);
                        delete signing.prompt;
                        if (error !== undefined) reject(error);
                        else resolve(value ?? "");
                    };
                    signing.prompt = {
                        resolve: (value) => {
                            signing.secrets.push(value);
                            finish(value);
                        },
                        reject: (error) => finish(undefined, error),
                    };
                    if (stop.aborted) return aborted();
                    stop.addEventListener("abort", aborted, { once: true });
                    const options =
                        prompt.type === "select"
                            ? `\n${prompt.options.map((option) => `${option.id}: ${option.label}`).join("\n")}`
                            : "";
                    send({
                        prompt: prompt.message + options,
                        secret:
                            prompt.type === "secret" ||
                            prompt.type === "manual_code",
                    });
                });
            signing.job = (async () => {
                try {
                    const outcome = await home.modelHome.login(
                        message.start.provider,
                        message.start.method,
                        {
                            signal,
                            prompt,
                            notify: (event) => {
                                switch (event.type) {
                                    case "auth_url":
                                        send({
                                            url: event.url,
                                            ...(event.instructions === undefined
                                                ? {}
                                                : {
                                                      instructions:
                                                          event.instructions,
                                                  }),
                                        });
                                        break;
                                    case "device_code":
                                        send({ url: event.verificationUri });
                                        send({ code: event.userCode });
                                        break;
                                    case "info":
                                        for (const link of event.links ?? [])
                                            send({ url: link.url });
                                        send({ progress: event.message });
                                        break;
                                    case "progress":
                                        send({ progress: event.message });
                                        break;
                                }
                            },
                        },
                    );
                    if (!outcome.ok) {
                        send({ failed: loginFailureText(outcome.error) });
                        return;
                    }
                    await refresh();
                    send({ done: message.start.provider });
                } finally {
                    signing.prompt?.reject(new Error("Sign-in ended."));
                    sessions.delete(id);
                }
            })().catch((error: unknown) => {
                if (!client.signal.aborted) client.fail(error, message.id);
            });
            return undefined;
        }
        const id = "reply" in message ? message.reply.login : message.cancel;
        const signing = sessions.get(id);
        if (signing === undefined || signing.client !== client)
            return err({
                kind: "login-unknown",
                login: id,
                operation: "sign-in",
                retrySafe: false,
            });
        if ("cancel" in message) signing.controller.abort();
        else {
            if (signing.prompt === undefined)
                return err({
                    kind: "login-not-waiting",
                    login: id,
                    operation: "sign-in",
                    retrySafe: false,
                });
            signing.prompt.resolve(message.reply.value);
        }
        return ok({
            text: "cancel" in message ? "Sign-in cancelled." : "Sent.",
        });
    };
    return {
        handle,
        stop: async () => {
            for (const signing of sessions.values()) signing.controller.abort();
            await Promise.all(
                [...sessions.values()].map((signing) => signing.job),
            );
        },
    };
}

export {
    type ChiefFailure,
    faultFor,
    type Receipt,
    replyFor,
} from "./server/envelope.ts";
