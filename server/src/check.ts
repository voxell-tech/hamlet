import type { Context } from "@earendil-works/chord";
import type { Home } from "./home.ts";
import { plainly } from "./plainly.ts";
import { safeWords } from "./scrub.ts";
import { modelRef, SettingsDoc, settingsOf } from "./settings.ts";

/** A model name resolved without pi's ambiguous provider/model spelling. */
export function namedModel(home: Home, name: string) {
    const ref = modelRef(name);
    if (ref === undefined)
        throw new Error(
            `\`${name}\` names no endpoint. A model is named \`endpoint::model\``,
        );
    if (home.models.getProvider(ref.provider) === undefined)
        throw new Error(
            `\`${name}\` is served by the endpoint \`${ref.provider}\`, which the settings do not have`,
        );
    const model = home.models.getModel(ref.provider, ref.modelId);
    if (model === undefined) throw new Error(`no model named \`${name}\``);
    return model;
}

/** A one-word hello, bounded at 90 seconds, with the outcome kept in the link. */
export async function checkModel(
    home: Home,
    name: string | undefined,
    context: Context,
): Promise<string> {
    const settings = settingsOf(
        await home.harness.snapshot(SettingsDoc, context),
        home.overrides,
    );
    const model = namedModel(home, name ?? settings.model);
    await link(home, "testing", null, null, context);
    const controller = new AbortController();
    const signal =
        context.abortSignal === undefined
            ? controller.signal
            : AbortSignal.any([controller.signal, context.abortSignal]);
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
        const answer = await Promise.race([
            home.models.completeSimple(
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
            ),
            new Promise<never>((_resolve, reject) => {
                timer = setTimeout(() => {
                    controller.abort();
                    reject(new Error("the model did not answer in time"));
                }, 90_000);
            }),
        ]);
        const text = answer.content
            .flatMap((part) => (part.type === "text" ? [part.text] : []))
            .join("")
            .trim();
        if (
            answer.stopReason === "error" ||
            answer.stopReason === "aborted" ||
            text === ""
        )
            throw new Error(
                answer.errorMessage ?? "the model answered with nothing",
            );
        await link(home, "connected", null, text, context);
        return text;
    } catch (error) {
        const text = plainly(safeWords(home, String(error)), true);
        await link(home, "failed", null, text, context);
        return text;
    } finally {
        clearTimeout(timer);
    }
}

/** A fresh listing from the default model's endpoint, with plain connection words. */
export async function checkConnection(
    home: Home,
    context: Context,
): Promise<string> {
    const settings = settingsOf(
        await home.harness.snapshot(SettingsDoc, context),
        home.overrides,
    );
    const model = namedModel(home, settings.model);
    await link(home, "testing", null, null, context);
    const result = await home.models.refresh({
        providers: [model.provider],
        force: true,
        signal: AbortSignal.timeout(10_000),
    });
    const error = result.errors.get(model.provider);
    const count = home.models.getModels(model.provider).length;
    const words =
        error !== undefined
            ? plainly(safeWords(home, error.message))
            : result.aborted
              ? plainly("in time")
              : `Connected, ${count} model(s)`;
    await link(
        home,
        error !== undefined || result.aborted ? "failed" : "connected",
        error !== undefined ? null : count,
        words,
        context,
    );
    return words;
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
