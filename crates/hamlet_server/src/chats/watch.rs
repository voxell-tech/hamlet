//! A chief's watch of a chat: its transcript as it stands, older
//! pages, and the history of its sessions (C24, S5).

use anyhow::Result;
use hamlet_protocol::v0::{
    Address, ChatRef, ChatRefKind, Open, ServerMessage, Transcript,
};
use serde_json::json;

use super::{Watch, records};
use crate::harness::{BindingId, Harnesses, link};
use crate::protocol::{fault, reply};
use crate::service::Service;
use crate::society::letters::{Kind, peer_of};

/// Items a transcript page holds.
const PAGE: usize = 40;

/// The key a client names a chat by.
pub(super) fn key(chat: &ChatRef) -> String {
    serde_json::to_string(chat).unwrap_or_default()
}

/// Watches `open.chat` for `client`, or gives an older page of a chat
/// it watches.
pub(super) async fn open(
    service: &mut Service,
    harnesses: &mut Harnesses,
    client: u64,
    open: &Open,
) -> Result<Option<ServerMessage>> {
    let chat = &open.chat;
    let binding = match resolve(service, harnesses, chat).await? {
        Ok(binding) => binding,
        Err(words) => return Ok(Some(fault(Some(&open.id), words))),
    };
    let watched = service.chats.watches.iter().any(|watch| {
        watch.client == client && key(&watch.chat) == key(chat)
    });
    if open.before.is_some() && watched {
        return Ok(Some(transcript(
            service,
            binding,
            chat,
            open.before.as_deref(),
            Some(&open.id),
        )?));
    }
    let message = transcript(
        service,
        binding,
        chat,
        open.before.as_deref(),
        Some(&open.id),
    )?;
    if matches!(message, ServerMessage::Transcript(_)) {
        let usage = super::usage(service, binding)?;
        super::conversation(service, binding)?.usage = Some(usage);
        let chats = &mut service.chats;
        chats.watches.retain(|watch| {
            watch.client != client || key(&watch.chat) != key(chat)
        });
        chats.watches.push(Watch {
            client,
            chat: chat.clone(),
            binding,
        });
    }
    Ok(Some(message))
}

/// The conversation `chat` names: the current one of the chief's chat
/// with its agent, which its first use starts, or a past one by
/// number. Else the words that refuse it.
pub(super) async fn resolve(
    service: &mut Service,
    harnesses: &mut Harnesses,
    chat: &ChatRef,
) -> Result<std::result::Result<BindingId, String>> {
    if let Err(refusal) =
        service.access(&chat.with.hamlet, "watch chat")?
    {
        return Ok(Err(refusal.text()));
    }
    if let Some(from) = &chat.from {
        let agent =
            match service.agent(&chat.with, "watch conversations")? {
                Ok(agent) => agent,
                Err(refusal) => return Ok(Err(refusal.text())),
            };
        if chat.past.is_some() {
            return Ok(Err("A peer conversation has no \
                           chief-session number."
                .into()));
        }
        let kind = match chat.kind {
            Some(ChatRefKind::Questions) => Kind::Questions,
            _ => Kind::Letters,
        };
        let agent = crate::record::AgentRef {
            hamlet: chat.with.hamlet.clone(),
            number: agent.number,
        };
        let from = crate::record::AgentRef {
            hamlet: from.hamlet.clone(),
            number: u64::try_from(from.number)?,
        };
        return Ok(peer_of(
            service.home.store().connection(),
            &agent,
            kind,
            &from,
        )?
        .ok_or_else(|| {
            "There is no conversation with that agent here.".into()
        }));
    }
    let agent = match service.agent(&chat.with, "watch chat")? {
        Ok(agent) => agent,
        Err(refusal) => return Ok(Err(refusal.text())),
    };
    let agent = crate::record::AgentRef {
        hamlet: chat.with.hamlet.clone(),
        number: agent.number,
    };
    let Some(number) = chat.past else {
        return Ok(Ok(
            super::current(service, harnesses, &agent).await?
        ));
    };
    let chief = service.chief.clone();
    Ok(
        match records::session(
            service.home.store().connection(),
            &agent,
            &chief,
            number,
        )? {
            Some(session) => Ok(session.binding),
            None => Err(format!(
                "There is no past session `{number}` here."
            )),
        },
    )
}

/// The page of `chat`, the conversation of `binding`, ending before
/// the item `before`, or at its end.
pub(super) fn transcript(
    service: &mut Service,
    binding: BindingId,
    chat: &ChatRef,
    before: Option<&str>,
    to: Option<&str>,
) -> Result<ServerMessage> {
    let usage = super::usage(service, binding)?;
    let number =
        records::kept(service.home.store().connection(), binding)?
            .map(|kept| kept.place.session);
    let conversation = super::conversation(service, binding)?;
    let fold = &conversation.fold;
    let end = match before {
        None => fold.items.len(),
        Some(before) => {
            match fold.items.iter().position(|item| item.id == before)
            {
                Some(end) => end,
                None => {
                    return Ok(fault(
                        to,
                        format!("There is no item `{before}` here."),
                    ));
                }
            }
        }
    };
    let start = end.saturating_sub(PAGE);
    Ok(ServerMessage::Transcript(Transcript {
        chat: chat.clone(),
        earlier: (start > 0).then(|| fold.items[start].id.clone()),
        items: fold.items[start..end].to_vec(),
        last: Some(fold.last.clone()),
        number: chat.from.is_none().then_some(number).flatten(),
        pending: fold.pending,
        to: to.map(String::from),
        usage,
        working: fold.working,
    }))
}

/// The past and current sessions of the chief's chat with `agent`,
/// newest first, each named by its first line (C24).
pub(super) async fn history(
    service: &mut Service,
    _harnesses: &mut Harnesses,
    address: &Address,
    id: &str,
) -> Result<ServerMessage> {
    let agent = match service.agent(address, "read or edit papers")? {
        Ok(agent) => agent,
        Err(refusal) => return Ok(reply(id, Err(refusal))),
    };
    let agent = crate::record::AgentRef {
        hamlet: address.hamlet.clone(),
        number: agent.number,
    };
    let past = pasts(service, &agent)?;
    Ok(serde_json::from_value(
        json!({"type": "read", "to": id, "history": past}),
    )?)
}

/// The sessions of the chief's chat with `agent` that hold a chief's
/// line, newest first.
pub(super) fn pasts(
    service: &mut Service,
    agent: &crate::record::AgentRef,
) -> Result<Vec<hamlet_protocol::v0::Past>> {
    let chief = service.chief.clone();
    let db = service.home.store().connection();
    let current = records::current_number(db, agent, &chief)?;
    let mut history = Vec::new();
    for session in records::sessions(db, agent, &chief)? {
        let items = service.home.store().items(session.binding)?;
        let Some((first, by)) = items.iter().find_map(|item| {
            let mark = item.mark.as_ref()?;
            let text = mark.data.get("text")?.as_str()?;
            (mark.kind == "hamlet.chief" && !text.trim().is_empty())
                .then(|| {
                    (
                        text.to_owned(),
                        mark.data
                            .get("chief")
                            .and_then(|c| c.as_str())
                            .unwrap_or_default()
                            .to_owned(),
                    )
                })
        }) else {
            continue;
        };
        let line = first
            .split('\n')
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default()
            .trim();
        let mut title = line.chars().take(90).collect::<String>();
        if line.chars().count() > 90 {
            title.push_str("...");
        }
        let messages = items
            .iter()
            .filter(|item| {
                item.mark
                    .as_ref()
                    .is_some_and(|mark| mark.kind == "hamlet.chief")
                    || (item.kind == link::ItemKind::Assistant
                        && item.blocks.iter().flatten().any(|block| {
                            matches!(block, link::Block::Text { text }
                                if !text.trim().is_empty())
                        }))
            })
            .count();
        let modified = items
            .iter()
            .filter_map(|item| i64::try_from(item.at).ok())
            .chain([session.began])
            .max()
            .unwrap_or(session.began);
        let usage = super::usage(service, session.binding)?;
        history.push(hamlet_protocol::v0::Past {
            began: session.began,
            by,
            current: Some(session.number) == current,
            messages: i64::try_from(messages)?,
            modified,
            number: session.number,
            title,
            tokens: usage.input + usage.output,
        });
    }
    history.sort_by_key(|past| std::cmp::Reverse(past.number));
    Ok(history)
}
