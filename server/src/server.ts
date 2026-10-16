import type { Context } from "@earendil-works/chord";
import { withAbortSignal } from "@earendil-works/chord/context";
import type { Submission } from "@earendil-works/pi-durable";
import {
    type ChatId,
    carryOn,
    resume,
    start,
    stop as stopConversation,
    takeBack,
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
import { checkConnection, checkModel } from "./models.ts";
import type { Address, ServerMessage } from "./protocol.ts";
import { chiefAccess, legacyRefusal } from "./realm/access.ts";
import { livingAgent } from "./realm/roads.ts";
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
import { loginSessions } from "./server/sign-in.ts";
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
        (error) =>
            console.error(
                home.secrets.words(
                    `hamlet: could not show the realm: ${String(error)}`,
                ),
            ),
    );
    // Each client's open chats, by the chat as it named them.
    const watching = new Map<Client, Map<string, ChatWatch>>();
    const stop = async (client: Client, chat: string) => {
        const watches = watching.get(client);
        await watches?.get(chat)?.stop();
        watches?.delete(chat);
    };
    const changed = async (
        outcome: string | Result<string | Receipt, ChiefFailure>,
    ): Promise<Result<Receipt, ChiefFailure>> => {
        if (typeof outcome !== "string" && !outcome.ok) return outcome;
        await state.refresh();
        return ok({
            ...(typeof outcome === "string"
                ? { text: outcome }
                : typeof outcome.value === "string"
                  ? { text: outcome.value }
                  : outcome.value),
        });
    };
    const current = async (
        address: Address,
    ): Promise<Result<ChatId, ChiefFailure>> => {
        const target = await livingAgent(home, address, "change chat", context);
        return target.ok ? ok(target.value.chat) : target;
    };
    await clearStagedGifts(home);
    const gifts = uploads(home, context);
    const logins = loginSessions(home, () => state.refresh());
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
                    return replyFor(message.id, outcome, home.secrets, "typed");
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
            login: (message, client) => logins.handle(message, client),
            check: (message, client) => {
                void (async () => {
                    // D9 replaces these model-check compatibility catches with model failures.
                    const outcome = await changed(
                        message.what === "connection"
                            ? await checkConnection(home, context).catch(
                                  legacyRefusal,
                              )
                            : await checkModel(
                                  home,
                                  message.model,
                                  context,
                              ).catch(legacyRefusal),
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
                // D8 replaces these gift-call compatibility catches with GiftRefusal.
                if (!("files" in message))
                    return replyFor(
                        message.id,
                        await changed(
                            await give(home, message, context).catch(
                                legacyRefusal,
                            ),
                        ),
                        home.secrets,
                    );
                return replyFor(
                    message.id,
                    await gifts
                        .begin(message, client.signal)
                        .catch(legacyRefusal),
                    home.secrets,
                );
            },
            // D8 replaces this gift-part compatibility catch with GiftRefusal.
            "give-part": async (message, client) =>
                replyFor(
                    message.id,
                    await changed(
                        await gifts
                            .part(message, client.signal)
                            .catch(legacyRefusal),
                    ),
                    home.secrets,
                ),
            // D9/D11 replace this edit-call compatibility catch with settings and limits failures.
            edit: async (message) =>
                replyFor(
                    message.id,
                    await changed(
                        await edit(home, message, context).catch(legacyRefusal),
                    ),
                    home.secrets,
                ),
            read: async (message) => {
                const outcome = await read(home, message, context);
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
                if (message.agent === undefined)
                    return replyFor(
                        message.id,
                        await changed(
                            await stopConversation(home, "all", context),
                        ),
                        home.secrets,
                    );
                const chat = await current(message.agent);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                return replyFor(
                    message.id,
                    await changed(
                        await stopConversation(home, chat.value.agent, context),
                    ),
                    home.secrets,
                );
            },
            new: async (message) => {
                const chat = await current(message.to);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                return replyFor(
                    message.id,
                    await changed(await start(home, chat.value, context)),
                    home.secrets,
                );
            },
            resume: async (message) => {
                const chat = await current(message.to);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                return replyFor(
                    message.id,
                    await changed(
                        await resume(home, chat.value, message.number, context),
                    ),
                    home.secrets,
                );
            },
            retry: async (message) => {
                const chat = await current(message.to);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                return replyFor(
                    message.id,
                    await changed(await carryOn(home, chat.value, context)),
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
                const writable = requireWritable(message.chat);
                if (!writable.ok)
                    return replyFor(message.id, writable, home.secrets);
                const chat = await current(message.chat.with);
                if (!chat.ok) return replyFor(message.id, chat, home.secrets);
                return replyFor(
                    message.id,
                    await changed(
                        await takeBack(home, chat.value, message.item, context),
                    ),
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

export {
    type ChiefFailure,
    faultFor,
    type Receipt,
    replyFor,
} from "./server/envelope.ts";
