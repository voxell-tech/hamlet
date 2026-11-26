/**
 * A client's calls of an agent. Each checks that a client called and
 * answers a value; only `askedBy` makes one.
 */
import {
    type AgentCall,
    type AgentConfig,
    generationOf,
    rowAt,
    wrongCaller,
} from "./actor.ts";
import { newSession, reflectNow, resume, retry, rewind } from "./commands.ts";
import { rotationsKept, secretsLoaded } from "./credentials.ts";
import {
    type Checked,
    type ClientActions,
    readClient,
} from "./messages/agent.ts";
import {
    agentKeys,
    checkConnection,
    checkModel,
    modelList,
    providerViews,
    reached,
    runningImages,
    runningModel,
    signedInTo,
} from "./models.ts";
import { editMemory, readPapers } from "./papers.ts";
import { promptSections } from "./prompt.ts";
import { isEndpoint } from "./providers.ts";
import { readChat, readHistory } from "./transcript.ts";

declare const asked: unique symbol;

export type Asked<Args extends readonly unknown[], R> = ((
    c: AgentCall,
    ...args: Args
) => Promise<R>) & { readonly [asked]: true };

export type ChatHandlers = {
    readonly [N in keyof ClientActions]: Asked<
        Parameters<ClientActions[N]>,
        ReturnType<ClientActions[N]>
    >;
};

/**
 * Actions that answer only fixed words or the realm's, so they do not wait
 * on the realm's secret list and are taken in the order they come.
 */
const ANSWERS_NO_READ: ReadonlySet<keyof ClientActions> = new Set([
    "retry",
    "settle",
]);

export function askedBy<N extends keyof ClientActions>(
    action: N,
    handle: (
        c: AgentCall,
        ...args: Parameters<ClientActions[N]>
    ) => Promise<ReturnType<ClientActions[N]>>,
): ChatHandlers[N] {
    const read = readClient[action];
    const askedOf = async (c: AgentCall, ...raw: unknown[]) => {
        if (c.caller !== "client") throw wrongCaller(action, c.caller);
        if (!ANSWERS_NO_READ.has(action)) {
            await secretsLoaded(generationOf(c).peers.reach);
        }
        return handle(c, ...read(raw));
    };
    return askedOf as unknown as ChatHandlers[N];
}

export function chatActions(config: AgentConfig): ChatHandlers {
    return {
        new_session: askedBy("new_session", newSession),
        resume: askedBy("resume", resume),
        retry: askedBy("retry", retry),
        rewind: askedBy("rewind", rewind),
        read_chat: askedBy("read_chat", readChat),
        read_history: askedBy("read_history", readHistory),
        read_prompt: askedBy("read_prompt", async (c) => ({
            sections: await promptSections(generationOf(c).prompt),
        })),
        reflect: askedBy("reflect", reflectNow),
        login: askedBy("login", async (c, login) => {
            if ((await rowAt(c)).kind !== "hand") {
                return { type: "not_the_hand" };
            }
            if (config.signIns === undefined) {
                throw new Error("this host signs no one in");
            }
            return config.signIns.step(
                login,
                generationOf(c).peers.reach.keep_credential,
            );
        }),
        settle: askedBy("settle", async () => {
            await Promise.all([rotationsKept(), config.signIns?.close()]);
            return null;
        }),
        read_models: askedBy("read_models", async (c, { refresh }) => {
            if (config.faux) return { models: modelList(config.models) };
            if (refresh === true)
                await config.models.refresh({
                    providers: config.models
                        .getProviders()
                        .filter(isEndpoint)
                        .map((provider) => provider.id),
                    force: true,
                });
            return {
                models: modelList(
                    config.models,
                    await reached(config.models, generationOf(c).peers.reach),
                ),
            };
        }),
        images_taken: askedBy("images_taken", async (c) => {
            const taken = await runningImages(
                config.models,
                generationOf(c).copies,
                config.model,
            );
            return taken === undefined ? {} : { per_line: taken.perLine };
        }),
        check: askedBy("check", async (c, { model }) => {
            const { peers, copies } = generationOf(c);
            const name = model ?? (await runningModel(copies, config.model));
            return config.faux && !signedInTo(config.models, name)
                ? FAUX_CHECKED
                : checkModel(
                      config.models,
                      agentKeys(config.models, peers),
                      name,
                  );
        }),
        check_connection: askedBy("check_connection", async (c) =>
            config.faux
                ? FAUX_CHECKED
                : checkConnection(
                      config.models,
                      await runningModel(generationOf(c).copies, config.model),
                  ),
        ),
        read_providers: askedBy("read_providers", async (c) => ({
            providers: await providerViews(
                config.models,
                generationOf(c).peers.reach,
            ),
        })),
        read_papers: askedBy("read_papers", readPapers),
        edit_memory: askedBy("edit_memory", editMemory),
    };
}

/** A check's answer where the faux model runs, as today's faux realm answers every check. */
const FAUX_CHECKED: Checked = {
    type: "answered",
    words: "Every agent runs the faux model here.",
};
