//! The lines a chief types that are dealt with before any model sees
//! them: a command about what waits, and words to an agent that waits
//! for the chief.

use bevy_ecs::prelude::*;
use hamlet_home::limits::{Limits, Permission};
use hamlet_rules::chief::rulings::{self, Choice, Command};

use super::grants::Grants;
use super::{Answer, Kind, Revoke, Veto, Waiting};
use crate::agents;
use crate::session::{SessionKey, Typed, UserMessage};

/// The most of what waits that the head of its strip quotes, in
/// characters.
const STRIP_CHARS: usize = 60;

/// The most of what waits that a reply to the chief quotes, in
/// characters.
const SAID_CHARS: usize = 80;

/// One thing that waits, in full, with what may be answered.
pub fn strip(waiting: &Waiting) -> String {
    let answers = waiting
        .answers
        .iter()
        .map(|c| format!("/{}", c.word()))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "[{}] {}\n    {}\n    answer with: {answers}",
        waiting.kind.name(),
        waiting.line(STRIP_CHARS),
        waiting.about.replace('\n', "\n    "),
    )
}

/// The fate of a plain line typed to an agent that has something
/// waiting.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Plain {
    /// It goes to the agent as any message does.
    Send,
    /// It is the chief's reply to the agent's question.
    Reply,
    /// It is held back: the agent waits for an answer that words
    /// alone cannot give.
    Hold,
}

/// A question wants words, so a line answers it. An approval or an
/// objection wants a decision, and a line is never read as one.
fn plain(open: Option<Kind>) -> Plain {
    match open {
        Some(Kind::Question) => Plain::Reply,
        Some(Kind::Approval(_) | Kind::Objection(_)) => Plain::Hold,
        Some(Kind::Fault | Kind::Lapsed) | None => Plain::Send,
    }
}

/// The holders of each permission, by the limits and by grants.
fn permissions(limits: Option<&Limits>, grants: &Grants) -> String {
    let mut lines = Vec::new();
    if let Some(limits) = limits {
        for permission in Permission::ALL {
            for name in permission.standing(&limits.permissions) {
                lines.push(format!(
                    "{name} may use: {} (limits.toml)",
                    permission.name()
                ));
            }
        }
    }
    for grant in grants.held() {
        lines.push(format!(
            "{} may use: {} (given by {}; /revoke {} {})",
            grant.name,
            grant.permission.name(),
            grant.by,
            grant.name,
            grant.permission.name()
        ));
    }
    if lines.is_empty() {
        lines.push("No agent holds a permission.".into());
    }
    lines.join("\n")
}

/// Deals with a chief's line, if it is one for this module.
pub(super) fn typed(
    mut typed: On<Typed>,
    waiting: Query<(Entity, &Waiting)>,
    sessions: Query<(Entity, &SessionKey)>,
    grants: Res<Grants>,
    world: &World,
    mut commands: Commands,
) {
    let line = typed.event_mut();
    let to = line.key.address().to_string();
    let session = sessions
        .iter()
        .find(|(_, key)| **key == line.key)
        .map(|(e, _)| e);
    let mut queue = waiting.iter().collect::<Vec<_>>();
    queue.sort_by_key(|(_, w)| w.order);
    // The thing waiting at the line's addressee comes first.
    let theirs = queue.iter().copied().find(|(_, w)| {
        w.by == to || session.is_some() && w.session == session
    });
    let chief = line.user.clone();

    let read = line
        .ruling
        .take()
        .map(|(ruling, rest)| rulings::read(ruling, &rest));
    let command = match read {
        Some(Ok(command)) => command,
        Some(Err(usage)) => {
            line.reply = Some(usage);
            return;
        }
        None => {
            let Some((entity, open)) = theirs else {
                return;
            };
            match plain(Some(open.kind)) {
                Plain::Send => {}
                Plain::Reply => {
                    commands.trigger(Answer {
                        entity,
                        choice: Choice::Reply,
                        text: line.text.trim().to_string(),
                        chief,
                    });
                    line.reply = Some(format!(
                        "Sent to {} as your answer to: {}",
                        open.by,
                        open.line(SAID_CHARS)
                    ));
                }
                Plain::Hold => {
                    line.reply = Some(format!(
                        "Not sent. {} is waiting for your \
                         answer:\n{}",
                        open.by,
                        strip(open)
                    ));
                }
            }
            return;
        }
    };
    line.reply = Some(match command {
        Command::Queue if queue.is_empty() => {
            "Nothing waits for you.".into()
        }
        Command::Queue => queue
            .iter()
            .map(|(_, w)| {
                format!(
                    "{} (waited {}s, asked {})",
                    strip(w),
                    w.waited().as_secs(),
                    w.asked
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Command::Answer { choice, text } => {
            match theirs.or(queue.first().copied()) {
                None => "Nothing waits for you.".into(),
                Some((_, item))
                    if !item.answers.contains(&choice) =>
                {
                    format!(
                        "That cannot be answered that way:\n{}",
                        strip(item)
                    )
                }
                Some((entity, item)) => {
                    commands.trigger(Answer {
                        entity,
                        choice,
                        text,
                        chief,
                    });
                    format!(
                        "Answered /{}: {}",
                        choice.word(),
                        item.line(SAID_CHARS)
                    )
                }
            }
        }
        Command::Stop { agent } => {
            let said = match &agent {
                Some(name) => format!("Stopping {name}."),
                None => "Stopping everything.".into(),
            };
            commands.trigger(Veto { agent, chief });
            said
        }
        Command::Revoke { agent, permission } => {
            let standing =
                agents::limits(world).is_ok_and(|limits| {
                    permission
                        .standing(&limits.permissions)
                        .contains(&agent)
                });
            let said = if standing {
                format!(
                    "Revoked what was granted. `{agent}` still has \
                     {} under [permissions] in limits.toml.",
                    permission.name()
                )
            } else {
                format!(
                    "`{agent}` no longer has {}.",
                    permission.name()
                )
            };
            commands.trigger(Revoke {
                agent,
                permission,
                chief,
            });
            said
        }
        Command::Grants => {
            permissions(agents::limits(world).ok().as_ref(), &grants)
        }
        Command::Undo => match super::hamlet(world).archived().last()
        {
            Some(gone) => {
                let name = gone.resident.name.clone();
                // The hamlet's own command does the restoring.
                commands.trigger(UserMessage {
                    key: line.key.clone(),
                    user: chief,
                    text: format!("/restore {name}"),
                });
                String::new()
            }
            None => "Nothing to undo. Only a removal or a merge can \
                     be undone (the archive keeps the house); a \
                     permission is taken back with /revoke. A \
                     command that ran, a file that was changed and \
                     what was spent cannot be undone."
                .into(),
        },
        Command::To { agent, text } => {
            commands.trigger(UserMessage {
                key: SessionKey::with_agent(
                    &line.key.channel,
                    &line.key.chat,
                    &agent,
                ),
                user: chief,
                text,
            });
            String::new()
        }
    });
}
