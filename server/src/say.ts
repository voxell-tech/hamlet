import type { Context } from "@earendil-works/chord";
import type { Submission } from "@earendil-works/pi-durable";
import { ChatDoc, submitLine } from "./chats.ts";
import { readCommand } from "./commands.ts";
import { folder, setMode } from "./environment.ts";
import { answerWords, GrantsDoc, grantsOf, revoke } from "./gate.ts";
import { historyOf } from "./history.ts";
import type { Deed, Home } from "./home.ts";
import { giftSkill } from "./house.ts";
import { resetBudget } from "./ledger.ts";
import * as library from "./library.ts";
import { LimitsDoc, limitsOf } from "./limits.ts";
import { OversightDoc, queue, strip } from "./oversight.ts";
import type { Reply, Say } from "./protocol.ts";
import * as founding from "./realm.ts";
import { chiefAccess, chiefRefusalText, resolve } from "./realm.ts";
import { addressed, agentKey } from "./record.ts";
import { reportOf } from "./report.ts";
import { safeWords } from "./scrub.ts";
import * as agents from "./society/agents.ts";
import * as leaving from "./society/leaving.ts";
import * as merging from "./society/merging.ts";
import * as voting from "./society/votes.ts";
import {
    newChat,
    reflect,
    resumeChat,
    retry,
    stopAgent,
    stopChiefAgents,
} from "./talk.ts";

/** A chief's typed line, dispatched as a command or admitted as scrubbed words (H24, C32). */
export async function say(
    home: Home,
    message: Say,
    context: Context,
    submitted?: (submission: Submission) => void,
): Promise<Reply | undefined> {
    const access = await chiefAccess(home, message.to.hamlet, "say", context);
    if (!access.ok) return reply(message.id, chiefRefusalText(access), "error");
    const text = safeWords(home, message.text);
    const command = readCommand(text);
    if (command !== undefined && "error" in command)
        return reply(message.id, command.error, command.kind);
    if (command?.command === "deed") {
        try {
            const deed = DEED_HANDLERS[command.deed];
            if (!deed)
                return reply(message.id, "Choose that address in your client.");
            return reply(
                message.id,
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
        } catch (error) {
            return reply(
                message.id,
                safeWords(
                    home,
                    error instanceof Error ? error.message : String(error),
                ),
                "error",
            );
        }
    }
    if (command?.command === "to") {
        const target = await resolve(
            home.harness,
            message.to.hamlet,
            command.agent,
            context,
        );
        if (!target) throw new Error(`no agent named \`${command.agent}\``);
        if ("gone" in target) throw new Error(target.gone);
        const access = await chiefAccess(
            home,
            target.hamlet,
            "instruct",
            context,
        );
        if (!access.ok)
            return reply(message.id, chiefRefusalText(access), "error");
        const sub = await submitLine(
            home,
            { agent: target.ref, chief: home.chief },
            { id: message.id, text: command.text },
            context,
        );
        submitted?.(sub);
        return undefined;
    }
    if (command?.command === "budget") {
        const report = await reportOf(home, message.to.hamlet, context);
        return reply(message.id, [report.summary, ...report.lines].join("\n"));
    }
    if (command?.command === "reset-budget")
        return reply(
            message.id,
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
        return reply(
            message.id,
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
        return reply(
            message.id,
            limits.permissions[command.permission].includes(command.agent)
                ? `Revoked what was granted. \`${command.agent}\` still has ${command.permission} under [permissions] in limits.toml.`
                : `\`${command.agent}\` no longer has ${command.permission}.`,
        );
    }
    if (
        command?.command === "folder" ||
        command?.command === "mode" ||
        command?.command === "give"
    ) {
        try {
            const text =
                command.command === "folder"
                    ? await folder(
                          home,
                          message.to.hamlet,
                          command.path,
                          context,
                      )
                    : command.command === "mode"
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
            return typeof text === "string" || text.ok
                ? reply(
                      message.id,
                      typeof text === "string" ? text : text.value,
                  )
                : reply(message.id, chiefRefusalText(text), "error");
        } catch (error) {
            return reply(
                message.id,
                safeWords(
                    home,
                    error instanceof Error ? error.message : String(error),
                ),
                "error",
            );
        }
    }
    if (command?.command === "stop") {
        if (command.agent === null) {
            await stopChiefAgents(home, context);
            return reply(message.id, "Stopping everything.");
        }
        const target = await resolve(
            home.harness,
            message.to.hamlet,
            command.agent,
            context,
        );
        if (!target)
            return reply(
                message.id,
                `no agent named \`${command.agent}\``,
                "error",
            );
        if ("gone" in target) return reply(message.id, target.gone, "error");
        const access = await chiefAccess(home, target.hamlet, "stop", context);
        if (!access.ok)
            return reply(message.id, chiefRefusalText(access), "error");
        await stopAgent(home, target.ref, context);
        return reply(message.id, `Stopping ${command.agent}.`);
    }
    const target = await resolve(
        home.harness,
        message.to.hamlet,
        `${message.to.agent}@${message.to.hamlet}`,
        context,
    );
    if (target && "gone" in target) throw new Error(target.gone);
    const agent = target && "ref" in target ? target.ref : undefined;
    if (
        agent !== undefined &&
        (command?.command === "new" ||
            command?.command === "resume" ||
            command?.command === "reflect" ||
            command?.command === "retry")
    ) {
        const chat = { agent, chief: home.chief };
        try {
            switch (command.command) {
                case "new":
                    await newChat(home, chat, context);
                    return reply(message.id, "Started a new session.");
                case "resume":
                    if (command.number === null) {
                        const past = (
                            await historyOf(home, chat, context)
                        ).filter((each) => !each.current);
                        return reply(
                            message.id,
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
                        return reply(
                            message.id,
                            `There is no past session \`${command.number}\` here.`,
                        );
                    await resumeChat(home, chat, command.number, context);
                    return reply(
                        message.id,
                        "Went back to an earlier session.",
                    );
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
                        return reply(
                            message.id,
                            "the last message was answered, so there is nothing to carry on",
                        );
                    {
                        const continued = await retry(home, chat, context);
                        submitted?.(continued);
                    }
                    return undefined;
            }
        } catch (error) {
            return reply(
                message.id,
                safeWords(
                    home,
                    error instanceof Error ? error.message : String(error),
                ),
            );
        }
    }
    const waits = queue(await home.harness.snapshot(OversightDoc, context));
    const chat =
        agent === undefined
            ? undefined
            : (await home.harness.snapshot(ChatDoc, agentKey(agent), context))
                  ?.chats[home.chief];
    const own =
        waits.find(
            (wait) =>
                agent !== undefined &&
                wait.chat?.hamlet === agent.hamlet &&
                wait.chat.number === agent.number &&
                wait.chat.chief === home.chief &&
                wait.chat.chat === chat?.current,
        ) ??
        waits.find(
            (wait) =>
                agent !== undefined &&
                wait.agent?.hamlet === agent.hamlet &&
                wait.agent.number === agent.number,
        );
    if (command?.command === "queue")
        return reply(
            message.id,
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
        if (wait === undefined)
            return reply(message.id, "Nothing waits for you.");
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
            return outcome.ok
                ? reply(message.id, outcome.value)
                : reply(message.id, chiefRefusalText(outcome), "error");
        } catch (error) {
            return reply(
                message.id,
                safeWords(
                    home,
                    error instanceof Error ? error.message : String(error),
                ),
            );
        }
    }
    if (!command && own?.kind === "question") {
        const outcome = await answerWords(
            home,
            { wait: own.id, choice: "reply", text, chief: home.chief },
            context,
        );
        return outcome.ok
            ? reply(message.id, outcome.value)
            : reply(message.id, chiefRefusalText(outcome), "error");
    }
    if (
        !text.trimStart().startsWith("/") &&
        own !== undefined &&
        (own.kind === "approval" || own.kind === "objection")
    )
        return reply(
            message.id,
            `Not sent. ${own.by} is waiting for your answer:\n${strip(own)}`,
        );
    if (agent === undefined) {
        throw new Error(`no agent named \`${message.to.agent}\``);
    }
    const submission = await submitLine(
        home,
        { agent, chief: home.chief },
        { id: message.id, text },
        context,
    );
    submitted?.(submission);
    return undefined;
}

function reply(
    to: string,
    text: string,
    kind: "notice" | "error" = "notice",
): Reply {
    return { type: "reply", to, kind, text };
}

const DEED_HANDLERS: Record<string, Deed> = {
    road: agents.road,
    unroad: agents.unroad,
    remove: leaving.remove,
    restore: leaving.restore,
    erase: leaving.erase,
    record: leaving.record,
    undo: leaving.undo,
    merge: merging.merge,
    vote: voting.vote,
    votes: voting.votes,
    overturn: voting.overturn,
    mayor: voting.mayor,
    unshelve: library.unshelve,
    found: founding.found,
    hamlets: founding.hamlets,
    "name-hamlet": founding.nameHamlet,
    "move-hamlet": founding.moveHamlet,
    "remove-hamlet": founding.removeHamletDeed,
    "restore-hamlet": founding.restoreHamlet,
    "erase-hamlet": founding.eraseHamlet,
};
