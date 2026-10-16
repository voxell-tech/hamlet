import type { Context } from "@earendil-works/chord";
import type { Submission } from "@earendil-works/pi-durable";
import * as agents from "./agents.ts";
import { submitLine } from "./chats/conversations.ts";
import {
    carryOn,
    currentNumber,
    history,
    reflect,
    resume,
    start,
    stop,
} from "./chats.ts";
import { readCommand } from "./commands.ts";
import { folder, setMode } from "./environment.ts";
import {
    answerFailureText,
    answerWords,
    GrantsDoc,
    grantsOf,
    revoke,
} from "./gate.ts";
import type { Deed, Home } from "./home.ts";
import { giftSkill } from "./house.ts";
import * as library from "./library.ts";
import { LimitsDoc, limitsOf } from "./limits/rules.ts";
import { forAgent, forChat, open } from "./oversight/waits.ts";
import { strip } from "./oversight/words.ts";
import type { Say } from "./protocol.ts";
import { chiefAccess, legacyRefusal } from "./realm/access.ts";
import { livingAgent } from "./realm/roads.ts";
import * as founding from "./realm.ts";
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
            // D4 replaces this deed-call compatibility catch with RecordRefusal.
            await deed(
                home,
                {
                    hamlet: message.to.hamlet,
                    rest: command.rest,
                    chief: home.chief,
                },
                context,
            ).catch(legacyRefusal),
        );
    }
    if (command?.command === "to") {
        const target = await livingAgent(
            home,
            { from: message.to.hamlet, text: command.agent },
            "instruct",
            context,
        );
        if (!target.ok) return target;
        const sub = await submitLine(
            home,
            target.value.chat,
            { id: message.id, text: command.text },
            context,
        );
        submitted?.(sub);
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
        const target = await livingAgent(
            home,
            { hamlet, agent: command.agent },
            "revoke",
            context,
        );
        const agent = target.ok
            ? target.value.ref
            : grantsOf(await home.harness.snapshot(GrantsDoc, context)).find(
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
        const text =
            command.command === "mode"
                ? await setMode(home, message.to.hamlet, command.mode, context)
                : await giftSkill(
                      home,
                      { hamlet: message.to.hamlet, agent: command.agent },
                      command.skill,
                      command.from === undefined
                          ? undefined
                          : { hamlet: message.to.hamlet, agent: command.from },
                      context,
                  );
        return receipt(text);
    }

    if (command?.command === "stop") {
        if (command.agent === null) return stop(home, "all", context);
        const living = await livingAgent(
            home,
            { from: message.to.hamlet, text: command.agent },
            "stop",
            context,
        );
        if (!living.ok) return living;
        const outcome = await stop(home, living.value.ref, context);
        return outcome.ok ? receipt(`Stopping ${command.agent}.`) : outcome;
    }
    const target = await livingAgent(home, message.to, "say", context);
    if (!target.ok && target.error.kind !== "agent-missing") return target;
    const agent = target.ok ? target.value.ref : undefined;
    if (
        agent !== undefined &&
        (command?.command === "new" ||
            command?.command === "resume" ||
            command?.command === "reflect" ||
            command?.command === "retry")
    ) {
        const chat = { agent, chief: home.chief };

        switch (command.command) {
            case "new":
                return start(home, chat, context);
            case "resume": {
                if (command.number !== null)
                    return resume(home, chat, command.number, context);
                const past = (await history(home, chat, context)).filter(
                    (each) => !each.current,
                );
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
            case "reflect": {
                const outcome = await reflect(home, chat, context, submitted);
                return outcome.ok ? undefined : outcome;
            }
            case "retry": {
                const outcome = await carryOn(home, chat, context, submitted);
                return outcome.ok ? undefined : outcome;
            }
        }
    }
    const waits = await open(home.harness, context);
    const number =
        agent === undefined
            ? undefined
            : await currentNumber(home, { agent, chief: home.chief }, context);
    const own =
        agent === undefined
            ? undefined
            : (forChat(waits, { agent, chief: home.chief }).find(
                  (wait) => wait.chat?.chat === number,
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
        return !outcome.ok && outcome.error.kind === "answer-not-offered"
            ? receipt(answerFailureText(outcome.error))
            : receipt(outcome);
    }
    if (!command && own?.kind === "question") {
        const outcome = await answerWords(
            home,
            { wait: own.id, choice: "reply", text, chief: home.chief },
            context,
        );
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
        if (!target.ok) return target;
        throw new Error("Resolved agent disappeared");
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

function receipt(
    outcome: string | Result<string, ChiefFailure>,
): Result<Receipt, ChiefFailure> {
    if (typeof outcome === "string") return ok({ text: outcome });
    return outcome.ok ? ok({ text: outcome.value }) : outcome;
}

const DEED_HANDLERS: Record<string, Deed> = {
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
    unshelve: library.unshelve,
    found: founding.found,
    hamlets: founding.hamlets,
    "name-hamlet": founding.nameHamlet,
    "move-hamlet": founding.moveHamlet,
    "remove-hamlet": founding.removeHamletDeed,
    "restore-hamlet": founding.restoreHamlet,
    "erase-hamlet": founding.eraseHamlet,
};
