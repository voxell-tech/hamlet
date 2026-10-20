//! Streaming terminal conversations and the chief's typed lines.

use std::collections::{HashMap, HashSet};
use std::io::{IsTerminal, Write};

use anyhow::{Context, Result, bail};
use hamlet_protocol::v0::*;
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

use crate::client::{self, Client, ServerAddress, reply_to};

const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "new",
        "",
        "Start a new session and keep this one in history",
    ),
    (
        "resume",
        "<number>",
        "Go back to a past session, by its number in History",
    ),
    (
        "retry",
        "",
        "Carry on a turn that failed or was stopped, from where it \
         ended",
    ),
    ("agent", "<name>", "Talk to that agent, in whichever hamlet"),
    ("hamlet", "<name>", "Talk to that hamlet's mayor"),
    ("to", "<agent> <instruction>", "Instruct an agent directly"),
    ("stop", "[agent|all]", "Stop an agent's work, or everyone's"),
    ("queue", "", "List what waits for you"),
    ("allow", "[reason]", "Allow what waits, this once"),
    ("always", "[reason]", "Allow it, and the like from now on"),
    ("deny", "[reason]", "Refuse what waits"),
    ("dismiss", "", "Put away a fault or a lapsed request"),
    ("undo", "", "Reverse the latest thing that can be reversed"),
    ("grants", "", "List what agents are allowed for good"),
    (
        "revoke",
        "<agent> <network|unconfined>",
        "Take back a lasting permission",
    ),
    (
        "remove",
        "<agent> [reason]",
        "Remove an agent and archive its house",
    ),
    (
        "merge",
        "<absorbed> into <kept> [reason]",
        "Merge one agent into another",
    ),
    (
        "road",
        "<agent> <agent> [reason]",
        "Build a road between two agents",
    ),
    (
        "unroad",
        "<agent> <agent> [reason]",
        "Take up the road between two agents",
    ),
    (
        "give",
        "<agent> <skill> [from <agent>]",
        "Give an agent a starter skill, or another's",
    ),
    (
        "folder",
        "<folder|none>",
        "Point this hamlet at a folder to work on, or at none",
    ),
    (
        "mode",
        "<manual|auto>",
        "Whether agents ask before they change the folder",
    ),
    (
        "reset-budget",
        "",
        "Start this hamlet's budget again from nothing spent",
    ),
    (
        "restore",
        "<agent> [as <new-name>] [reason]",
        "Bring back a removed or merged agent",
    ),
    (
        "erase",
        "<agent>",
        "Delete a removed agent for good, to free its room on disk",
    ),
    (
        "record",
        "",
        "Who was created, removed, merged and restored",
    ),
    ("budget", "", "The hamlet's budget and each agent's spend"),
    (
        "reflect",
        "",
        "Have the agent reflect on this conversation now",
    ),
    (
        "mayor",
        "<agent> [reason]",
        "Make an agent of this hamlet its mayor",
    ),
    (
        "vote",
        "<agent> [reason]",
        "Ask this hamlet's agents to vote on making that agent \
         their mayor",
    ),
    ("votes", "", "This hamlet's votes, with every ballot"),
    (
        "overturn",
        "<vote> [reason]",
        "Reverse a vote that passed, or stop an open one",
    ),
    ("hamlets", "", "List the hamlets and their mayors"),
    (
        "found",
        "<hamlet> <mayor> <purpose>",
        "Found a hamlet of your own under a new mayor",
    ),
    (
        "remove-hamlet",
        "<hamlet> [reason]",
        "Remove a hamlet and archive its houses",
    ),
    ("restore-hamlet", "<hamlet>", "Bring a removed hamlet back"),
    (
        "unshelve",
        "<skill> [reason]",
        "Take a skill out of the library",
    ),
    (
        "erase-hamlet",
        "<hamlet>",
        "Delete a removed hamlet for good",
    ),
    (
        "name-hamlet",
        "<hamlet> <new-name>",
        "Call a hamlet something else",
    ),
    (
        "move-hamlet",
        "<hamlet> <x> <z>",
        "Set a hamlet's town down somewhere else",
    ),
    ("chief", "", "The commands about hamlets and votes"),
];

fn id() -> String {
    Uuid::new_v4().to_string()
}
fn address_key(address: &Address) -> String {
    format!("{}@{}", address.agent, address.hamlet)
}
fn chat_ref(with: Address) -> ChatRef {
    ChatRef {
        with,
        from: None,
        kind: None,
        past: None,
    }
}

fn resolve(
    state: &State,
    name: &str,
    hamlet: Option<&str>,
) -> Result<Address> {
    if let Some(hamlet) = hamlet {
        anyhow::ensure!(
            state.hamlets.iter().any(|h| h.id == hamlet),
            "no hamlet named `{hamlet}`"
        );
    }
    let agent = state
        .agents
        .iter()
        .find(|a| {
            a.left.is_none()
                && hamlet.is_none_or(|h| a.ref_.hamlet == h)
                && if name == "mayor" {
                    a.mayor
                } else {
                    a.name == name
                }
        })
        .with_context(|| format!("no agent named `{name}`"))?;
    Ok(Address {
        hamlet: agent.ref_.hamlet.clone(),
        agent: if agent.mayor {
            "mayor".into()
        } else {
            agent.name.clone()
        },
    })
}

async fn open(
    client: &mut Client,
    opens: &mut HashSet<String>,
    id: String,
    chat: ChatRef,
) -> Result<()> {
    opens.insert(id.clone());
    client
        .send(ClientMessage::Open(Open {
            id,
            chat,
            before: None,
        }))
        .await
}

/// Chats until EOF has received every submitted line's reply;
/// disconnecting stops no agent.
pub async fn run(address: &ServerAddress, to: &str) -> Result<()> {
    let (mut client, mut state) = client::connect(address).await?;
    let parts = to.split('@').collect::<Vec<_>>();
    anyhow::ensure!(
        parts.len() <= 2 && parts.iter().all(|s| !s.is_empty()),
        "--to names an agent, name@hamlet or mayor@hamlet"
    );
    let mut target = resolve(
        &state,
        parts[0],
        parts.get(1).copied().or(if parts[0] == "mayor" {
            Some("home")
        } else {
            None
        }),
    )?;
    let mut screen = Screen::default();
    screen.show(&ServerMessage::State(state.clone()))?;
    eprintln!(
        "hamlet: Ctrl-D quits after your lines are answered; Ctrl-C \
         quits at once and stops no agent"
    );
    for (deed, args, description) in COMMANDS {
        if state
            .deeds
            .as_ref()
            .is_none_or(|deeds| deeds.iter().any(|name| name == deed))
        {
            eprintln!(
                "  /{}: {description}",
                format!("{deed} {args}").trim_end()
            );
        }
    }
    if state.settings.unset {
        eprintln!(
            "Set up a provider with `hamlet login <provider> \
             [--key]`, then try a hello with `hamlet models try`."
        );
    }
    let mut opens = HashSet::new();
    open(&mut client, &mut opens, id(), chat_ref(target.clone()))
        .await?;
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let interactive = std::io::stdin().is_terminal();
    let mut prompted = false;
    let mut last = None;
    let mut final_id = None;
    let mut ending = false;
    let mut pending = HashSet::new();
    let mut numbers = HashMap::new();
    let mut pinned = HashSet::new();
    loop {
        let message = tokio::select! {
            line = input.next_line(), if last.is_none() => {
                match line? {
                    None => {
                        let barrier = id();
                        open(&mut client, &mut opens, barrier.clone(), chat_ref(target.clone())).await?;
                        last = Some(barrier);
                    },
                    Some(line) if line.trim().is_empty() => prompt(&mut screen, interactive)?,
                    Some(line) => {
                        let trimmed = line.trim();
                        let (command, name) = trimmed.split_once(char::is_whitespace).unwrap_or((trimmed, ""));
                        if command == "/agent" || command == "/hamlet" {
                            match switch(&state, &target, command, name.trim()) {
                                Ok(next) => {
                                    if next != target {
                                        target = next;
                                        open(&mut client, &mut opens, id(), chat_ref(target.clone())).await?;
                                    }
                                    eprintln!("  (talking to the {} {})", if command == "/agent" { "agent" } else { "mayor of" }, name.trim());
                                },
                                Err(error) => eprintln!("error: {error}"),
                            }
                            prompt(&mut screen, interactive)?;
                        } else {
                            let id = id();
                            pending.insert(id.clone());
                            client.send(ClientMessage::Say(Say { id, to: target.clone(), text: line })).await?;
                        }
                    },
                }
                continue;
            },
            message = client.next() => message?,
        };
        if let ServerMessage::Fault(fault) = &message
            && fault.to.as_ref().is_some_and(|id| opens.contains(id))
        {
            bail!("{}", fault.text);
        }
        match &message {
            ServerMessage::State(next) => state = next.clone(),
            ServerMessage::Change(changes) => {
                for change in &changes.changes {
                    match change {
                        Update::Hamlet { hamlet } => {
                            state
                                .hamlets
                                .retain(|h| h.id != hamlet.id);
                            state.hamlets.push(hamlet.clone());
                        }
                        Update::Agent { agent } => {
                            state
                                .agents
                                .retain(|a| a.ref_ != agent.ref_);
                            state.agents.push(agent.clone());
                        }
                        Update::AgentGone { ref_ } => {
                            state.agents.retain(|a| a.ref_ != *ref_)
                        }
                        _ => {}
                    }
                }
            }
            ServerMessage::Transcript(transcript) => {
                if let Some(to) = &transcript.to {
                    opens.remove(to);
                }
                if transcript.chat.past.is_none()
                    && let Some(number) = transcript.number
                    && let Some(previous) = numbers.insert(
                        address_key(&transcript.chat.with),
                        number,
                    )
                {
                    let pin = (
                        address_key(&transcript.chat.with),
                        previous,
                    );
                    if previous != number && pinned.insert(pin) {
                        let mut chat =
                            chat_ref(transcript.chat.with.clone());
                        chat.past = Some(previous);
                        open(&mut client, &mut opens, id(), chat)
                            .await?;
                    }
                }
            }
            _ => {}
        }
        let chat = match &message {
            ServerMessage::Transcript(t) => Some(&t.chat),
            ServerMessage::Turn(t) => Some(&t.chat),
            _ => None,
        };
        let duplicate = chat.is_some_and(|chat| {
            chat.past.is_some()
                && chat.past
                    == numbers.get(&address_key(&chat.with)).copied()
        });
        if !duplicate {
            screen.show(&message)?;
        }
        if let ServerMessage::Transcript(transcript) = &message {
            if !prompted {
                prompted = true;
                prompt(&mut screen, interactive && last.is_none())?;
            }
            if last.is_some() && transcript.to == last {
                ending = true;
            }
        }
        if matches!(
            message,
            ServerMessage::Reply(_) | ServerMessage::Fault(_)
        ) && reply_to(&message)
            .is_some_and(|id| pending.remove(id))
        {
            prompt(&mut screen, interactive && last.is_none())?;
        }
        if ending && pending.is_empty() && final_id.is_none() {
            let final_barrier = id();
            open(
                &mut client,
                &mut opens,
                final_barrier.clone(),
                chat_ref(target.clone()),
            )
            .await?;
            final_id = Some(final_barrier);
        }
        if let ServerMessage::Transcript(transcript) = message
            && final_id.is_some()
            && transcript.to == final_id
        {
            break;
        }
    }
    screen.end()?;
    client.close().await
}

fn switch(
    state: &State,
    target: &Address,
    command: &str,
    name: &str,
) -> Result<Address> {
    anyhow::ensure!(!name.is_empty(), "usage: {command} <name>");
    if command == "/agent" {
        return resolve(
            state,
            name,
            if name == "mayor" {
                Some(&target.hamlet)
            } else {
                None
            },
        );
    }
    let hamlet = state
        .hamlets
        .iter()
        .find(|h| h.id == name || h.name == name)
        .with_context(|| format!("no hamlet named `{name}`"))?;
    resolve(state, "mayor", Some(&hamlet.id))
}

fn prompt(screen: &mut Screen, interactive: bool) -> Result<()> {
    if interactive {
        screen.end()?;
        eprint!("\n> ");
        std::io::stderr().flush()?;
    }
    Ok(())
}

#[derive(Default)]
struct Screen {
    streamed: HashMap<String, (usize, bool)>,
    open: Option<(String, bool)>,
    waits: HashSet<String>,
    seen: HashSet<String>,
    opened: bool,
    views: HashMap<String, Vec<Item>>,
}

impl Screen {
    fn end(&mut self) -> Result<()> {
        if let Some((_, reasoning)) = self.open.take() {
            Self::write(reasoning, "\n")?;
        }
        Ok(())
    }

    fn write(reasoning: bool, text: &str) -> Result<()> {
        if reasoning {
            let mut out = std::io::stderr().lock();
            out.write_all(text.as_bytes())?;
            out.flush()?;
        } else {
            let mut out = std::io::stdout().lock();
            out.write_all(text.as_bytes())?;
            out.flush()?;
        }
        Ok(())
    }

    fn line(&mut self, text: &str) -> Result<()> {
        self.end()?;
        eprintln!("{text}");
        Ok(())
    }

    fn stream(&mut self, id: &str, text: &str) -> Result<()> {
        let Some((printed, reasoning)) = self.streamed.get_mut(id)
        else {
            return Ok(());
        };
        if text.is_empty() {
            return Ok(());
        }
        *printed += text.len();
        let reasoning = *reasoning;
        if self.open.as_ref().is_none_or(|(open, _)| open != id) {
            self.end()?;
        }
        Self::write(
            reasoning,
            &if reasoning {
                format!("\x1b[2m{text}\x1b[0m")
            } else {
                text.into()
            },
        )?;
        self.open = Some((id.into(), reasoning));
        Ok(())
    }

    fn add(
        &mut self,
        item: &Item,
        replaces: Option<&str>,
    ) -> Result<()> {
        if !self.seen.insert(item.id.clone()) {
            if let Some((printed, _)) = self.streamed.get(&item.id) {
                self.stream(
                    &item.id,
                    item.text.get(*printed..).unwrap_or_default(),
                )?;
            }
            return Ok(());
        }
        if let Some(replaces) = replaces {
            self.seen.remove(replaces);
        }
        match item.kind {
            ItemKind::Assistant | ItemKind::Reasoning => {
                let printed = replaces
                    .and_then(|id| self.streamed.remove(id))
                    .map(|(n, _)| n)
                    .unwrap_or(0);
                self.streamed.insert(
                    item.id.clone(),
                    (printed, item.kind == ItemKind::Reasoning),
                );
                if let Some((open, _)) = &mut self.open
                    && Some(open.as_str()) == replaces
                {
                    *open = item.id.clone();
                }
                self.stream(
                    &item.id,
                    item.text.get(printed..).unwrap_or_default(),
                )?;
            }
            ItemKind::Tool => {
                if let Some(tool) = &item.tool {
                    self.line(&format!(
                        "\n  [{}] {}",
                        tool.name, tool.summary
                    ))?;
                }
            }
            ItemKind::Notice => {
                self.line(&format!("\n  ({})", item.text))?
            }
            ItemKind::Error => {
                self.line(&format!("\nerror: {}", item.text))?
            }
            ItemKind::Answered => {
                self.line(&format!("\n  chief:\n{}", item.text))?
            }
            ItemKind::User => {}
        }
        Ok(())
    }

    fn wait(&mut self, wait: &Wait) -> Result<()> {
        if !self.waits.insert(wait.id.clone()) {
            return Ok(());
        }
        let clip = |text: &str| {
            if text.chars().count() <= 60 {
                text.into()
            } else {
                format!(
                    "{}…",
                    text.chars().take(59).collect::<String>()
                )
            }
        };
        let (by, subject) = (&wait.by, clip(&wait.subject));
        let (kind, line) = match &wait.detail {
            WaitDetail::Approval { says, .. } => {
                ("approval", format!("{by} {says} `{subject}`"))
            }
            WaitDetail::Objection {} => ("objection", subject),
            WaitDetail::Question { .. } => {
                ("question", format!("{by} asks: {subject}"))
            }
            WaitDetail::Fault {} => {
                ("fault", format!("{by} failed: {subject}"))
            }
        };
        let answers = wait
            .answers
            .iter()
            .map(|answer| {
                serde_json::to_value(answer).map(|v| {
                    format!("/{}", v.as_str().unwrap_or_default())
                })
            })
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join(" ");
        self.line(&format!(
            "\n  NEEDS YOU: [{kind}] {line}\n    {}\n    answer \
             with: {answers}",
            wait.about.replace('\n', "\n    ")
        ))
    }

    fn item_id(
        chat: &ChatRef,
        number: Option<i64>,
        id: &str,
    ) -> String {
        if id.starts_with("live:") {
            format!(
                "{}:{:?}:{id}",
                address_key(&chat.with),
                number.or(chat.past)
            )
        } else {
            id.into()
        }
    }

    fn show(&mut self, message: &ServerMessage) -> Result<()> {
        match message {
            ServerMessage::State(state) => {
                for wait in &state.waits {
                    self.wait(wait)?;
                }
            }
            ServerMessage::Change(changes) => {
                for change in &changes.changes {
                    if let Update::Wait { wait } = change {
                        self.wait(wait)?;
                    }
                }
            }
            ServerMessage::Fault(fault) => {
                self.line(&format!("\nerror: {}", fault.text))?
            }
            ServerMessage::Reply(reply) if !reply.text.is_empty() => {
                self.line(&if reply.kind == ReplyKind::Error {
                    format!("\nerror: {}", reply.text)
                } else {
                    format!("\n  ({})", reply.text)
                })?
            }
            ServerMessage::Transcript(t) => {
                let mut items = t.items.clone();
                for item in &mut items {
                    item.id =
                        Self::item_id(&t.chat, t.number, &item.id);
                }
                if !self.opened {
                    let last = items.iter().rposition(|item| {
                        item.kind == ItemKind::Assistant
                    });
                    for (i, item) in items.iter().enumerate() {
                        if Some(i) != last
                            && !(item.kind == ItemKind::Tool
                                && item.tool.as_ref().is_some_and(
                                    |t| t.exit.is_none(),
                                ))
                        {
                            self.seen.insert(item.id.clone());
                        }
                    }
                    self.opened = true;
                }
                for item in &items {
                    self.add(item, None)?;
                }
                self.views.insert(
                    format!(
                        "{}:{:?}",
                        serde_json::to_string(&t.chat)?,
                        t.number.or(t.chat.past)
                    ),
                    items,
                );
                if !t.working {
                    self.end()?;
                }
            }
            ServerMessage::Turn(t) => {
                let key = format!(
                    "{}:{:?}",
                    serde_json::to_string(&t.chat)?,
                    t.number.or(t.chat.past)
                );
                let mut items =
                    self.views.remove(&key).unwrap_or_default();
                for op in &t.ops {
                    match op {
                        Op::Add { item, replaces } => {
                            let mut item = item.clone();
                            item.id = Self::item_id(
                                &t.chat, t.number, &item.id,
                            );
                            let replaces =
                                replaces.as_ref().map(|s| {
                                    Self::item_id(
                                        &t.chat, t.number, s,
                                    )
                                });
                            if let Some(index) =
                                replaces.as_ref().and_then(|id| {
                                    items
                                        .iter()
                                        .position(|i| &i.id == id)
                                })
                            {
                                items[index] = item.clone();
                            } else {
                                items.push(item.clone());
                            }
                            self.add(&item, replaces.as_deref())?;
                        }
                        Op::Append { item, text } => {
                            let id = Self::item_id(
                                &t.chat, t.number, item,
                            );
                            if let Some(item) =
                                items.iter_mut().find(|i| i.id == id)
                            {
                                item.text.push_str(text);
                                let printed = self
                                    .streamed
                                    .get(&id)
                                    .map(|(n, _)| *n)
                                    .unwrap_or(0);
                                self.stream(
                                    &id,
                                    item.text
                                        .get(printed..)
                                        .unwrap_or_default(),
                                )?;
                            }
                        }
                        Op::Close { item, exit, .. } => {
                            let id = Self::item_id(
                                &t.chat, t.number, item,
                            );
                            if let Some(tool) = items
                                .iter_mut()
                                .find(|i| i.id == id)
                                .and_then(|i| i.tool.as_mut())
                            {
                                tool.exit = Some(*exit);
                                if *exit != 0 {
                                    self.line(&format!(
                                        "  [exit {exit}]"
                                    ))?;
                                }
                            }
                        }
                        Op::Status { working: false, .. } => {
                            self.end()?
                        }
                        Op::Reset {} => items.clear(),
                        _ => {}
                    }
                }
                self.views.insert(key, items);
            }
            _ => {}
        }
        Ok(())
    }
}
