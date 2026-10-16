import type { Context } from "@earendil-works/chord";
import type { AssistantMessage } from "@earendil-works/pi-ai";
import type { Home } from "../home.ts";
import { err, ok, type Result } from "../result.ts";
import { modelRef, SettingsDoc, settingsOf } from "../settings.ts";
import {
    type ModelFailure,
    modelFailureText,
    type ProviderFailure,
} from "./failure.ts";

/** Resolves only fully qualified model names at a configured provider. */
export function namedModel(
    home: Home,
    name: string,
): Result<NonNullable<ReturnType<Home["models"]["getModel"]>>, ModelFailure> {
    const facts = { operation: "check-model", retrySafe: true, name } as const;
    const ref = modelRef(name);
    if (ref === undefined) return err({ ...facts, kind: "model-name-invalid" });
    if (home.models.getProvider(ref.provider) === undefined)
        return err({
            ...facts,
            kind: "model-endpoint-missing",
            provider: ref.provider,
        });
    const model = home.models.getModel(ref.provider, ref.modelId);
    return model === undefined
        ? err({ ...facts, kind: "model-not-found" })
        : ok(model);
}

/** A hello bounded at 90 seconds; provider failures still update the shared link. */
export async function checkModel(
    home: Home,
    name: string | undefined,
    context: Context,
): Promise<Result<string, ModelFailure>> {
    const settings = settingsOf(
        await home.harness.snapshot(SettingsDoc, context),
        home.overrides,
    );
    const selected = namedModel(home, name ?? settings.model);
    if (!selected.ok) return selected;
    const model = selected.value;
    await link(home, "testing", null, null, context);
    const controller = new AbortController();
    const signal =
        context.abortSignal === undefined
            ? controller.signal
            : AbortSignal.any([controller.signal, context.abortSignal]);
    const failure = (reason: string): ProviderFailure => ({
        kind: "provider-refused",
        operation: "check-model",
        provider: model.provider,
        reason: home.secrets.words(reason),
        retrySafe: false,
    });
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
        const answer = await Promise.race([
            home.models
                .completeSimple(
                    model,
                    {
                        messages: [
                            {
                                role: "user",
                                content: "Say hello in one word.",
                                timestamp: home.now(),
                            },
                        ],
                    },
                    { signal, maxRetries: 0 },
                )
                .then(
                    (message) => ok(message),
                    (error: unknown) => err(failure(String(error))),
                ),
            new Promise<Result<AssistantMessage, ProviderFailure>>(
                (resolve) => {
                    timer = setTimeout(() => {
                        controller.abort();
                        resolve(
                            err(
                                failure(
                                    "Error: the model did not answer in time",
                                ),
                            ),
                        );
                    }, 90_000);
                },
            ),
        ]);
        if (!answer.ok) {
            await link(
                home,
                "failed",
                null,
                modelFailureText(answer.error),
                context,
            );
            return answer;
        }
        const text = answer.value.content
            .flatMap((part) => (part.type === "text" ? [part.text] : []))
            .join("")
            .trim();
        if (
            answer.value.stopReason === "error" ||
            answer.value.stopReason === "aborted" ||
            text === ""
        ) {
            const refused = failure(
                `Error: ${answer.value.errorMessage ?? "the model answered with nothing"}`,
            );
            await link(
                home,
                "failed",
                null,
                modelFailureText(refused),
                context,
            );
            return err(refused);
        }
        await link(home, "connected", null, text, context);
        return ok(text);
    } finally {
        clearTimeout(timer);
    }
}

/** A fresh serving-provider listing, retaining the check's notice wording on failure. */
export async function checkConnection(
    home: Home,
    context: Context,
): Promise<Result<string, ModelFailure>> {
    const settings = settingsOf(
        await home.harness.snapshot(SettingsDoc, context),
        home.overrides,
    );
    const selected = namedModel(home, settings.model);
    if (!selected.ok) return selected;
    const model = selected.value;
    await link(home, "testing", null, null, context);
    const result = await home.models.refresh({
        providers: [model.provider],
        force: true,
        signal: AbortSignal.timeout(10_000),
    });
    const error = result.errors.get(model.provider);
    const count = home.models.getModels(model.provider).length;
    const failure: ProviderFailure | undefined =
        error !== undefined || result.aborted
            ? {
                  kind: "provider-refused",
                  operation: "check-connection",
                  provider: model.provider,
                  reason: home.secrets.words(error?.message ?? "in time"),
                  retrySafe: false,
              }
            : undefined;
    const words =
        failure === undefined
            ? `Connected, ${count} model(s)`
            : modelFailureText(failure);
    await link(
        home,
        failure === undefined ? "connected" : "failed",
        error === undefined ? count : null,
        words,
        context,
    );
    return failure === undefined ? ok(words) : err(failure);
}

async function link(
    home: Home,
    state: "testing" | "connected" | "failed",
    models: number | null,
    words: string | null,
    context: Context,
): Promise<void> {
    await home.harness.commit(async (tx) => {
        (await tx.doc(SettingsDoc)).link = { state, models, words };
    }, context);
}

export { type ModelFailure, modelFailureText } from "./failure.ts";
