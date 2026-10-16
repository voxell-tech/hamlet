import { randomUUID } from "node:crypto";
import type { Context } from "@earendil-works/chord";
import { withAbortSignal } from "@earendil-works/chord/context";
import type { AuthPrompt } from "@earendil-works/pi-ai";
import type { Submission } from "@earendil-works/pi-durable";
import {
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
import { clearStagedGifts, give, uploads } from "./house.ts";
import { checkConnection, checkModel, login as signIn } from "./models.ts";
import type { Address, Login, LoginStep, ServerMessage } from "./protocol.ts";
import { type ChiefResult, chiefAccess } from "./realm/access.ts";
import { resolve } from "./realm/roads.ts";
import { ok, type Result } from "./result.ts";
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
        outcome: string | ChiefResult<string>,
    ): Promise<Result<Receipt, ChiefFailure>> => {
        if (typeof outcome !== "string" && !outcome.ok) return outcome;
        await state.refresh();
        return ok({
            text: typeof outcome === "string" ? outcome : outcome.value,
        });
    };
    const chatOf = async (address: Address) => {
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
        if (!target) throw new Error(`no agent named \`${address.agent}\``);
        if ("gone" in target) throw new Error(target.gone);
        return ok({ agent: target.ref, chief: home.chief });
    };
    await clearStagedGifts(home);
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
                            ? await checkConnection(home, context)
                            : await checkModel(home, message.model, context),
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
                const outcome = await read(home, message, context);
                return outcome.ok
                    ? home.secrets.value(outcome.value)
                    : replyFor(message.id, outcome, home.secrets);
            },
            answer: async (message) =>
                replyFor(
                    message.id,
                    await changed(
                        await answerWords(
                            home,
                            { ...message, chief: home.chief },
                            context,
                        ),
                    ),
                    home.secrets,
                ),
            stop: async (message) => {
                if (message.agent === undefined)
                    return replyFor(
                        message.id,
                        await changed(await stopChiefAgents(home, context)),
                        home.secrets,
                    );
                const chat = await chatOf(message.agent);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                return replyFor(
                    message.id,
                    await changed(
                        await stopAgent(home, chat.value.agent, context),
                    ),
                    home.secrets,
                );
            },
            new: async (message) => {
                const chat = await chatOf(message.to);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                await newChat(home, chat.value, context);
                return replyFor(
                    message.id,
                    await changed("Started a new session."),
                    home.secrets,
                );
            },
            resume: async (message) => {
                const chat = await chatOf(message.to);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                await resumeChat(home, chat.value, message.number, context);
                return replyFor(
                    message.id,
                    await changed("Went back to an earlier session."),
                    home.secrets,
                );
            },
            retry: async (message) => {
                const chat = await chatOf(message.to);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                await retry(home, chat.value, context);
                return replyFor(
                    message.id,
                    await changed("Carrying on from where it stopped."),
                    home.secrets,
                );
            },
            rewind: async (message) => {
                const chat = await chatOf(message.chat.with);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                requireWritable(message.chat);
                if (message.chat.past !== undefined)
                    throw new Error(
                        "Open the current conversation to rewind it.",
                    );
                const outcome = await changed(
                    await rewind(home, chat.value, message.item, context),
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
