import { randomUUID } from "node:crypto";
import type { Context } from "@earendil-works/chord";
import { withAbortSignal } from "@earendil-works/chord/context";
import type { AuthPrompt } from "@earendil-works/pi-ai";
import type { Submission } from "@earendil-works/pi-durable";
import { requireWritable } from "./agent-chats.ts";
import { checkConnection, checkModel } from "./check.ts";
import {
    type ChiefResult,
    chiefAccess,
    chiefRefusalText,
} from "./chief-access.ts";
import { type Client, type Server, serve } from "./connection.ts";
import { edit } from "./edits.ts";
import { answerWords } from "./gate.ts";
import { clearStagedGifts, give, uploads } from "./gifts.ts";
import type { Home } from "./home.ts";
import { login as signIn } from "./logins.ts";
import { safeValue, safeWords } from "./plainly.ts";
import type {
    Address,
    Login,
    LoginStep,
    Reply,
    ServerMessage,
} from "./protocol.ts";
import { read } from "./reads.ts";
import { resolve } from "./realm.ts";
import { say } from "./say.ts";
import { watchState } from "./state.ts";
import {
    newChat,
    resumeChat,
    retry,
    rewind,
    stopAgent,
    stopChiefAgents,
} from "./talk.ts";
import { type ChatWatch, pageChat, turnTools, watchChat } from "./turns.ts";

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
        (message) => server?.broadcast(safeValue(home, message)),
        context,
    );
    // Each client's open chats, by the chat as it named them.
    const watching = new Map<Client, Map<string, ChatWatch>>();
    const stop = async (client: Client, chat: string) => {
        const watches = watching.get(client);
        await watches?.get(chat)?.stop();
        watches?.delete(chat);
    };
    const deed = async (
        id: string,
        run: () => Promise<string | ChiefResult<string>>,
        address?: Address,
    ): Promise<Reply> => {
        try {
            if (address) {
                const access = await chiefAccess(
                    home,
                    address.hamlet,
                    "change chat",
                    context,
                );
                if (!access.ok)
                    return {
                        type: "reply",
                        to: id,
                        kind: "error",
                        text: safeWords(home, chiefRefusalText(access)),
                    };
            }
            const outcome = await run();
            if (typeof outcome !== "string" && !outcome.ok)
                return {
                    type: "reply",
                    to: id,
                    kind: "error",
                    text: safeWords(home, chiefRefusalText(outcome)),
                };
            const text = typeof outcome === "string" ? outcome : outcome.value;
            await state.refresh();
            return {
                type: "reply",
                to: id,
                kind: "notice",
                text: safeWords(home, text),
            };
        } catch (error) {
            return {
                type: "reply",
                to: id,
                kind: "error",
                text: safeWords(
                    home,
                    error instanceof Error ? error.message : String(error),
                ),
            };
        }
    };
    const chatOf = async (address: Address) => {
        const target = await resolve(
            home.harness,
            address.hamlet,
            `${address.agent}@${address.hamlet}`,
            context,
        );
        if (!target) throw new Error(`no agent named \`${address.agent}\``);
        if ("gone" in target) throw new Error(target.gone);
        return { agent: target.ref, chief: home.chief };
    };
    await clearStagedGifts(home);
    const gifts = uploads(home, context);
    const logins = loginConnection(home, () => state.refresh());
    server = await serve({
        token: home.credentials.token,
        port,
        handlers: {
            hello: async () => safeValue(home, await state.snapshot()),
            say: async (message, client) => {
                try {
                    let work: Submission | undefined;
                    const reply = await say(
                        home,
                        message,
                        context,
                        (submission) => {
                            work = submission;
                        },
                    );
                    await state.refresh();
                    if (reply !== undefined) return safeValue(home, reply);
                    const done = {
                        type: "reply",
                        to: message.id,
                        kind: "notice",
                        text: "",
                    } as const;
                    if (work === undefined) return done;
                    // This receipt settles the client's line, not the whole conversation.
                    void work
                        .wait(withAbortSignal(client.signal, context))
                        .then(() => {
                            if (!client.signal.aborted) client.send(done);
                        })
                        .catch((error: unknown) => {
                            if (!client.signal.aborted)
                                client.send({
                                    type: "fault",
                                    to: message.id,
                                    text: safeWords(home, String(error)),
                                });
                        });
                    return undefined;
                } catch (error) {
                    return deed(message.id, async () => {
                        throw error;
                    });
                }
            },
            open: async (message, client) => {
                try {
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
                            return {
                                type: "fault",
                                to: message.id,
                                text: safeWords(home, chiefRefusalText(page)),
                            };
                        return safeValue(home, {
                            ...page.value,
                            to: message.id,
                        });
                    }
                    const previous = watching.get(client)?.get(chat);
                    let buffered: ServerMessage[] | undefined = [];
                    const outcome = await watchChat(
                        home,
                        message.chat,
                        (message) => {
                            if (buffered !== undefined) buffered.push(message);
                            else if (!client.signal.aborted)
                                client.send(safeValue(home, message));
                        },
                        message.id,
                        context,
                        message.before,
                    );
                    if (!outcome.ok)
                        return {
                            type: "fault",
                            to: message.id,
                            text: safeWords(home, chiefRefusalText(outcome)),
                        };
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
                        for (const message of initial)
                            client.send(safeValue(home, message));
                    return undefined;
                } catch (error) {
                    const reply = await deed(message.id, async () => {
                        throw error;
                    });
                    return { type: "fault", to: message.id, text: reply.text };
                }
            },
            login: (message, client) => deedLogin(message, client),
            check: (message, client) => {
                void deed(message.id, () =>
                    message.what === "connection"
                        ? checkConnection(home, context)
                        : checkModel(home, message.model, context),
                ).then((reply) => {
                    if (!client.signal.aborted) client.send(reply);
                });
                return undefined;
            },
            give: async (message, client) => {
                if (!("files" in message))
                    return deed(message.id, () => give(home, message, context));
                try {
                    return safeValue(
                        home,
                        await gifts.begin(message, client.signal),
                    );
                } catch (error) {
                    return deed(message.id, async () => {
                        throw error;
                    });
                }
            },
            "give-part": (message, client) =>
                deed(message.id, () => gifts.part(message, client.signal)),
            edit: (message) =>
                deed(message.id, () =>
                    edit(home, safeValue(home, message), context),
                ),
            read: async (message) => {
                try {
                    return safeValue(home, await read(home, message, context));
                } catch (error) {
                    return deed(message.id, async () => {
                        throw error;
                    });
                }
            },
            answer: (message) =>
                deed(message.id, () =>
                    answerWords(
                        home,
                        { ...message, chief: home.chief },
                        context,
                    ),
                ),
            stop: (message) =>
                deed(
                    message.id,
                    async () =>
                        message.agent === undefined
                            ? stopChiefAgents(home, context)
                            : stopAgent(
                                  home,
                                  (await chatOf(message.agent)).agent,
                                  context,
                              ),
                    message.agent,
                ),
            new: (message) =>
                deed(
                    message.id,
                    async () => {
                        await newChat(home, await chatOf(message.to), context);
                        return "Started a new session.";
                    },
                    message.to,
                ),
            resume: (message) =>
                deed(
                    message.id,
                    async () => {
                        await resumeChat(
                            home,
                            await chatOf(message.to),
                            message.number,
                            context,
                        );
                        return "Went back to an earlier session.";
                    },
                    message.to,
                ),
            retry: (message) =>
                deed(
                    message.id,
                    async () => {
                        await retry(home, await chatOf(message.to), context);
                        return "Carrying on from where it stopped.";
                    },
                    message.to,
                ),
            rewind: async (message) => {
                const result = await deed(
                    message.id,
                    async () => {
                        requireWritable(message.chat);
                        if (message.chat.past !== undefined)
                            throw new Error(
                                "Open the current conversation to rewind it.",
                            );
                        return rewind(
                            home,
                            await chatOf(message.chat.with),
                            message.item,
                            context,
                        );
                    },
                    message.chat.with,
                );
                return result.kind === "error"
                    ? result
                    : { ...result, kind: "rewound" };
            },
            close: async (message, client) => {
                await stop(client, JSON.stringify(message.chat));
                return undefined;
            },
        },
    });
    async function deedLogin(message: Login, client: Client) {
        try {
            return await logins.handle(message, client);
        } catch (error) {
            return deed(message.id, async () => {
                throw error;
            });
        }
    }
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
    ): Promise<Reply | undefined> => {
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
                        safeValue(
                            home,
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
                    await signIn(
                        home,
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
                    await refresh();
                    send({ done: message.start.provider });
                } catch (error) {
                    send({
                        failed:
                            error instanceof Error
                                ? error.message
                                : String(error),
                    });
                } finally {
                    signing.prompt?.reject(new Error("Sign-in ended."));
                    sessions.delete(id);
                }
            })();
            return undefined;
        }
        const id = "reply" in message ? message.reply.login : message.cancel;
        const signing = sessions.get(id);
        if (signing === undefined || signing.client !== client)
            throw new Error("There is no sign-in here with that id.");
        if ("cancel" in message) signing.controller.abort();
        else {
            if (signing.prompt === undefined)
                throw new Error("That sign-in is not waiting for an answer.");
            signing.prompt.resolve(message.reply.value);
        }
        return {
            type: "reply",
            to: message.id,
            kind: "notice",
            text: "cancel" in message ? "Sign-in cancelled." : "Sent.",
        };
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
