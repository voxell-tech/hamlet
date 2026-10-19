// Transient provider interactions, owned by one server request.
import { randomUUID } from "node:crypto";
import type { AuthInteraction } from "@earendil-works/pi-ai";
import type { Channel } from "./channel.ts";
import type { ProviderControl } from "./link.ts";

/** One request's provider prompts and cancellation. */
export function signIn(
    channel: Channel,
    scope: string,
): {
    interaction: AuthInteraction;
    control(message: ProviderControl): void;
    stop(): void;
} {
    const controller = new AbortController();
    let waiting: { id: string; resolve(value: string): void } | undefined;
    const send = (step: Record<string, unknown>, prompt?: string) =>
        channel.send({
            type: "provider_event",
            scope,
            step,
            ...(prompt === undefined ? {} : { prompt }),
        });
    const interaction: AuthInteraction = {
        signal: controller.signal,
        prompt: (prompt) =>
            new Promise<string>((resolve, reject) => {
                const id = randomUUID();
                const signal =
                    prompt.signal === undefined
                        ? controller.signal
                        : AbortSignal.any([controller.signal, prompt.signal]);
                const finish = (value?: string) => {
                    signal.removeEventListener("abort", aborted);
                    waiting = undefined;
                    send({}, "");
                    if (value === undefined)
                        reject(new Error("Sign-in cancelled."));
                    else resolve(value);
                };
                const aborted = () => finish();
                if (signal.aborted) {
                    aborted();
                    return;
                }
                waiting = { id, resolve: (value) => finish(value) };
                signal.addEventListener("abort", aborted, { once: true });
                const options =
                    prompt.type === "select"
                        ? `\n${prompt.options.map((o) => `${o.id}: ${o.label}`).join("\n")}`
                        : "";
                send(
                    {
                        prompt: prompt.message + options,
                        secret:
                            prompt.type === "secret" ||
                            prompt.type === "manual_code",
                    },
                    id,
                );
            }),
        notify: (event) => {
            switch (event.type) {
                case "auth_url":
                    send({
                        url: event.url,
                        ...(event.instructions === undefined
                            ? {}
                            : { instructions: event.instructions }),
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
    };
    return {
        interaction,
        control: (message: ProviderControl) => {
            if (message.cancel) controller.abort();
            else if (
                message.prompt === waiting?.id &&
                message.value !== undefined
            )
                waiting?.resolve(message.value);
        },
        stop: () => controller.abort(),
    };
}
