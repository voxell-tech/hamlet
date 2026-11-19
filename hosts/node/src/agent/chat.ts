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
import {
    newSession,
    reflectNow,
    resume,
    retry,
    rewind,
    stop,
} from "./commands.ts";
import { type ClientActions, readClient } from "./messages.ts";
import { agentKeys, checkModel, modelList, secretsLoaded } from "./models.ts";
import { renderPrompt } from "./prompt.ts";
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
    "stop",
    "retry",
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
        stop: askedBy("stop", stop),
        new_session: askedBy("new_session", newSession),
        resume: askedBy("resume", resume),
        retry: askedBy("retry", retry),
        rewind: askedBy("rewind", rewind),
        read_chat: askedBy("read_chat", readChat),
        read_history: askedBy("read_history", readHistory),
        read_prompt: askedBy("read_prompt", async (c) => {
            return { text: await renderPrompt(generationOf(c).prompt) };
        }),
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
        read_models: askedBy("read_models", async () => ({
            models: modelList(config.models),
        })),
        check: askedBy("check", (c, { model }) =>
            checkModel(
                config.models,
                agentKeys(config.models, generationOf(c).peers),
                model,
            ),
        ),
    };
}
