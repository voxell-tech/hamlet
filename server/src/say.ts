import type { Context } from "@earendil-works/chord";
import type { Submission } from "@earendil-works/pi-durable";
import * as agents from "./agents.ts";
import { ChatDoc, submitLine } from "./chats/conversations.ts";
import {
    historyOf,
    newChat,
    reflect,
    resumeChat,
    retry,
    stopAgent,
    stopChiefAgents,
} from "./chats.ts";
import { readCommand } from "./commands.ts";
import { connectionFailureText } from "./connection.ts";
import { folder, setMode } from "./environment.ts";
import { answerWords, GrantsDoc, grantsOf, revoke } from "./gate.ts";
import type { Deed, Home } from "./home.ts";
import { giftSkill } from "./house.ts";
import * as library from "./library.ts";
import { LimitsDoc, limitsOf } from "./limits/rules.ts";
import { forAgent, forChat, open } from "./oversight/waits.ts";
import { strip } from "./oversight/words.ts";
import type { Say } from "./protocol.ts";
import { chiefAccess, legacyRefusal } from "./realm/access.ts";
import { resolve } from "./realm/roads.ts";
import * as founding from "./realm.ts";
import { addressed, agentKey } from "./record.ts";
import { err, ok, type Result } from "./result.ts";
import type { ChiefFailure, Receipt } from "./server.ts";
import { resetBudget } from "./spend/ledger.ts";
import { reportOf } from "./spend.ts";
import * as voting from "./votes.ts";

/** A chief's typed line: a receipt or refusal, or no immediate outcome when admitted as work. Stored words are scrubbed (H24, C32). */
export async function say(
    home: Home,
    message: Say,
    context: Context,
    submitted?: (submission: Submission) => void,
): Promise<Result<Receipt, ChiefFailure> | undefined> {
    const access = await chiefAccess(home, message.to.hamlet, "say", context);
    if (!access.ok) return access;
    const text = home.secrets.words(message.text);
    const command = readCommand(text);
    if (command !== undefined && "kind" in command) return err(command);
    if (command?.command === "deed") {
        const deed = DEED_HANDLERS[command.deed];
        if (!deed) return receipt("Choose that address in your client.");
        return receipt(
            await deed(
                home,
                {
                    hamlet: message.to.hamlet,
                    rest: command.rest,
                    chief: home.chief,
                },
                context,
            ),
        );
    }
    if (command?.command === "to") {
        const target = await resolve(
            home.harness,
            message.to.hamlet,
            command.agent,
            context,
        );
        if (!target)
            return err({ kind: "agent-missing", agent: command.agent });
        if ("gone" in target)
            return err({ kind: "agent-gone", farewell: target.gone });
        const access = await chiefAccess(
            home,
            target.hamlet,
            "instruct",
            context,
        );
        if (!access.ok) return access;
        // D3 replaces this line-admission compatibility catch with ConversationFailure.
        const sub = await submitLine(
            home,
            { agent: target.ref, chief: home.chief },
            { id: message.id, text: command.text },
            context,
        )
            .then(ok)
            .catch(legacyRefusal);
        if (!sub.ok) return sub;
        submitted?.(sub.value);
        return undefined;
    }
    if (command?.command === "budget") {
        const report = await reportOf(home, message.to.hamlet, context);
        return receipt([report.summary, ...report.lines].join("\n"));
    }
    if (command?.command === "reset-budget")
        return receipt(
            await resetBudget(home, message.to.hamlet, home.chief, context),
        );
    if (command?.command === "grants") {
        const limits = limitsOf(
            await home.harness.snapshot(LimitsDoc, message.to.hamlet, context),
        );
        const grants = grantsOf(
            await home.harness.snapshot(GrantsDoc, context),
        ).filter((grant) => grant.agent.hamlet === message.to.hamlet);
        const lines = Object.entries(limits.permissions).flatMap(
            ([permission, names]) =>
                names.map(
                    (name) => `${name} may use: ${permission} (limits.toml)`,
                ),
        );
        lines.push(
            ...grants.map(
                (grant) =>
                    `${grant.name} may use: ${grant.permission} (given by ${grant.by}; /revoke ${grant.name} ${grant.permission})`,
            ),
        );
        return receipt(
            lines.length === 0
                ? "No agent holds a permission."
                : lines.join("\n"),
        );
    }
    if (command?.command === "revoke") {
        const hamlet = message.to.hamlet;
        const agent =
            (await addressed(
                home.harness,
                { hamlet, agent: command.agent },
                context,
            )) ??
            grantsOf(await home.harness.snapshot(GrantsDoc, context)).find(
                (grant) =>
                    grant.agent.hamlet === hamlet &&
                    grant.name === command.agent,
            )?.agent;
        if (agent !== undefined)
            await home.harness.commit(
                (tx) =>
                    revoke(
                        tx,
                        agent,
                        command.permission,
                        home.chief,
                        home.now(),
                    ),
                context,
            );
        const limits = limitsOf(
            await home.harness.snapshot(LimitsDoc, hamlet, context),
        );
        return receipt(
            limits.permissions[command.permission].includes(command.agent)
                ? `Revoked what was granted. \`${command.agent}\` still has ${command.permission} under [permissions] in limits.toml.`
                : `\`${command.agent}\` no longer has ${command.permission}.`,
        );
    }
    if (command?.command === "folder") {
        const result = await folder(
            home,
            message.to.hamlet,
            command.path,
            context,
        );
        return result.ok ? receipt(result.value) : result;
    }
    if (command?.command === "mode" || command?.command === "give") {
        // D8 replaces this gift compatibility catch with GiftRefusal.
        try {
            const text =
                command.command === "mode"
                    ? await setMode(
                          home,
                          message.to.hamlet,
                          command.mode,
                          context,
                      )
                    : await giftSkill(
                          home,
                          { hamlet: message.to.hamlet, agent: command.agent },
                          command.skill,
                          command.from === undefined
                              ? undefined
                              : {
                                    hamlet: message.to.hamlet,
                                    agent: command.from,
                                },
                          context,
                      );
            return receipt(text);
        } catch (cause) {
            return legacyRefusal(cause);
        }
    }
    if (command?.command === "stop") {
        if (command.agent === null) {
            // D3 replaces this stop-call compatibility catch with ConversationFailure.
            return receipt(
                await stopChiefAgents(home, context).catch(legacyRefusal),
            );
        }
        const target = await resolve(
            home.harness,
            message.to.hamlet,
            command.agent,
            context,
        );
        if (!target)
            return err({ kind: "agent-missing", agent: command.agent });
        if ("gone" in target)
            return err({ kind: "agent-gone", farewell: target.gone });
        const access = await chiefAccess(home, target.hamlet, "stop", context);
        if (!access.ok) return access;
        // D3 replaces this stop-call compatibility catch with ConversationFailure.
        return receipt(
            await stopAgent(home, target.ref, context)
                .then(() => ok(`Stopping ${command.agent}.`))
                .catch(legacyRefusal),
        );
    }
    const target = await resolve(
        home.harness,
        message.to.hamlet,
        `${message.to.agent}@${message.to.hamlet}`,
        context,
    );
    if (target && "gone" in target)
        return err({ kind: "agent-gone", farewell: target.gone });
    const agent = target && "ref" in target ? target.ref : undefined;
    if (
        agent !== undefined &&
        (command?.command === "new" ||
            command?.command === "resume" ||
            command?.command === "reflect" ||
            command?.command === "retry")
    ) {
        const chat = { agent, chief: home.chief };

        // D3 replaces this conversation-call compatibility catch with ConversationFailure.
        try {
            switch (command.command) {
                case "new":
                    await newChat(home, chat, context);
                    return receipt("Started a new session.");
                case "resume":
                    if (command.number === null) {
                        const past = (
                            await historyOf(home, chat, context)
                        ).filter((each) => !each.current);
                        return receipt(
                            past.length === 0
                                ? "There are no past sessions here."
                                : past
                                      .map(
                                          (each) =>
                                              `${each.number}: ${each.title} (${each.messages} messages, ${each.tokens} tokens; ${new Date(each.began).toISOString()})`,
                                      )
                                      .join("\n"),
                        );
                    }
                    if (
                        !(
                            await home.harness.snapshot(
                                ChatDoc,
                                agentKey(agent),
                                context,
                            )
                        )?.chats[home.chief]?.conversations.some(
                            (each) => each.number === command.number,
                        )
                    )
                        return receipt(
                            `There is no past session \`${command.number}\` here.`,
                        );
                    await resumeChat(home, chat, command.number, context);
                    return receipt("Went back to an earlier session.");
                case "reflect":
                    {
                        const reflection = await reflect(home, chat, context);
                        if (reflection !== undefined) submitted?.(reflection);
                    }
                    return undefined;
                case "retry":
                    if (
                        !(
                            await home.harness.snapshot(
                                ChatDoc,
                                agentKey(agent),
                                context,
                            )
                        )?.chats[home.chief]
                    )
                        return receipt(
                            "the last message was answered, so there is nothing to carry on",
                        );
                    {
                        const continued = await retry(home, chat, context);
                        submitted?.(continued);
                    }
                    return undefined;
            }
        } catch (cause) {
            return receipt(connectionFailureText({ kind: "defect", cause }));
        }
    }
    const waits = await open(home.harness, context);
    const chat =
        agent === undefined
            ? undefined
            : (await home.harness.snapshot(ChatDoc, agentKey(agent), context))
                  ?.chats[home.chief];
    const own =
        agent === undefined
            ? undefined
            : (forChat(waits, { agent, chief: home.chief }).find(
                  (wait) => wait.chat?.chat === chat?.current,
              ) ?? forAgent(waits, agent)[0]);
    if (command?.command === "queue")
        return receipt(
            waits.length === 0
                ? "Nothing waits for you."
                : waits
                      .map(
                          (wait) =>
                              `${strip(wait)} (waited ${Math.floor(Math.max(0, home.now() - wait.since) / 1000)}s, asked ${wait.asked})`,
                      )
                      .join("\n"),
        );
    if (command?.command === "answer") {
        const wait = own ?? waits[0];
        if (wait === undefined) return receipt("Nothing waits for you.");

        // D6 replaces this answer-call compatibility catch with answer-not-offered.
        try {
            const outcome = await answerWords(
                home,
                {
                    wait: wait.id,
                    choice: command.choice,
                    text: command.text,
                    chief: home.chief,
                },
                context,
            );
            return receipt(outcome);
        } catch (cause) {
            return receipt(connectionFailureText({ kind: "defect", cause }));
        }
    }
    if (!command && own?.kind === "question") {
        // D6 replaces this plain-answer compatibility catch with an answer failure.
        const outcome = await answerWords(
            home,
            { wait: own.id, choice: "reply", text, chief: home.chief },
            context,
        ).catch(legacyRefusal);
        return receipt(outcome);
    }
    if (
        !text.trimStart().startsWith("/") &&
        own !== undefined &&
        (own.kind === "approval" || own.kind === "objection")
    )
        return receipt(
            `Not sent. ${own.by} is waiting for your answer:\n${strip(own)}`,
        );
    if (agent === undefined) {
        return err({ kind: "agent-missing", agent: message.to.agent });
    }
    // D3 replaces this line-admission compatibility catch with ConversationFailure.
    const submission = await submitLine(
        home,
        { agent, chief: home.chief },
        { id: message.id, text },
        context,
    )
        .then(ok)
        .catch(legacyRefusal);
    if (!submission.ok) return submission;
    submitted?.(submission.value);
    return undefined;
}

function receipt(
    outcome: string | Result<string, ChiefFailure>,
): Result<Receipt, ChiefFailure> {
    if (typeof outcome === "string") return ok({ text: outcome });
    return outcome.ok ? ok({ text: outcome.value }) : outcome;
}

const DEED_HANDLERS: Record<
    string,
    (...args: Parameters<Deed>) => Promise<Result<string, ChiefFailure>>
> = {
    road: agents.road,
    unroad: agents.unroad,
    remove: agents.remove,
    restore: agents.restore,
    erase: agents.erase,
    record: agents.record,
    undo: agents.undo,
    merge: agents.merge,
    vote: voting.vote,
    votes: voting.votes,
    overturn: voting.overturn,
    mayor: voting.mayor,
    // The library still reports its shelf refusals as exceptions.
    unshelve: (...args) =>
        library
            .unshelve(...args)
            .then(ok)
            .catch(legacyRefusal),
    found: founding.found,
    hamlets: founding.hamlets,
    "name-hamlet": founding.nameHamlet,
    "move-hamlet": founding.moveHamlet,
    "remove-hamlet": founding.removeHamletDeed,
    "restore-hamlet": founding.restoreHamlet,
    "erase-hamlet": founding.eraseHamlet,
};
