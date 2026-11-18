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
import { secretsLoaded } from "./models.ts";
import { promptSources, renderPrompt } from "./prompt.ts";
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
        await secretsLoaded(generationOf(c).peers.reach);
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
            const { copies, peers } = generationOf(c);
            return {
                text: await renderPrompt(promptSources(c, copies, peers)),
            };
        }),
        reflect: askedBy("reflect", reflectNow),
        login: askedBy("login", async (c, login) => {
            if (config.signIns === undefined) {
                throw new Error("this host signs no one in");
            }
            if ((await rowAt(c)).kind !== "hand") {
                throw new Error("only the Hand signs in");
            }
            return config.signIns.step(
                login,
                generationOf(c).peers.reach.keep_credential,
            );
        }),
    };
}
