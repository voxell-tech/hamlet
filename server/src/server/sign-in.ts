import { randomUUID } from "node:crypto";
import type { AuthPrompt } from "@earendil-works/pi-ai";
import type { Client } from "../connection.ts";
import type { Home } from "../home.ts";
import { login as signIn } from "../models.ts";
import type { Login, LoginStep, Reply } from "../protocol.ts";
import { legacyRefusal } from "../realm/access.ts";
import { ok, type Result } from "../result.ts";
import { type ChiefFailure, type Receipt, replyFor } from "./envelope.ts";

/** Owns each client's sign-ins and cancels their pending work before the server closes. */
export function loginSessions(home: Home, refresh: () => Promise<void>) {
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
        handle: async (
            message: Login,
            client: Client,
        ): Promise<Reply | undefined> => {
            // D9 replaces this sign-in-call compatibility catch with LoginFailure.
            const outcome = await handle(message, client).catch(legacyRefusal);
            return outcome === undefined
                ? undefined
                : replyFor(message.id, outcome, home.secrets);
        },
        stop: async () => {
            for (const signing of sessions.values()) signing.controller.abort();
            await Promise.all(
                [...sessions.values()].map((signing) => signing.job),
            );
        },
    };
}
