/**
 * The chief's commands to an agent: what each changes and how it is
 * refused. `chat.ts` makes each a client action.
 */
import type { AgentCall } from "./actor.ts";
import type {
    ClientActions,
    NewSession,
    Resume,
    ResumeAnswer,
    Retry,
    RetryAnswer,
    Rewind,
    RewindAnswer,
    Stop,
} from "./messages.ts";

type Answer<N extends keyof ClientActions> = Promise<
    ReturnType<ClientActions[N]>
>;

export function stop(c: AgentCall, message: Stop): Answer<"stop"> {
    void c;
    void message;
    return Promise.reject(new Error("not built: U4"));
}

export function newSession(
    c: AgentCall,
    message: NewSession,
): Answer<"new_session"> {
    void c;
    void message;
    return Promise.reject(new Error("not built: U2"));
}

export function resume(c: AgentCall, message: Resume): Promise<ResumeAnswer> {
    void c;
    void message;
    return Promise.reject(new Error("not built: U2"));
}

export function retry(c: AgentCall, message: Retry): Promise<RetryAnswer> {
    void c;
    void message;
    return Promise.reject(new Error("not built: U4"));
}

export function rewind(c: AgentCall, message: Rewind): Promise<RewindAnswer> {
    void c;
    void message;
    return Promise.reject(new Error("not built: U4"));
}

export function reflectNow(c: AgentCall): Answer<"reflect"> {
    void c;
    return Promise.reject(new Error("not built: U5"));
}
