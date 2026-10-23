//! Streaming terminal conversations and the chief's typed lines.

use std::collections::{HashMap, HashSet};
use std::io::{IsTerminal, Write};

use anyhow::{Context, Result, bail};
use hamlet_protocol::client::{transcript, *};
use hamlet_protocol::{HOME, MAYOR};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::client::{self, Client, ServerAddress};

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
                && if name == MAYOR {
                    a.mayor
                } else {
                    a.name == name
                }
        })
        .with_context(|| format!("no agent named `{name}`"))?;
    Ok(Address {
        hamlet: agent.ref_.hamlet.clone(),
        agent: if agent.mayor {
            MAYOR.into()
        } else {
            agent.name.clone()
        },
    })
}

/// Chats until EOF has received every submitted line's reply;
/// disconnecting stops no agent.
pub async fn run(address: &ServerAddress, to: &str) -> Result<()> {
    let (client, state) = client::connect(address).await?;
    let parts = to.split('@').collect::<Vec<_>>();
    anyhow::ensure!(
        parts.len() <= 2 && parts.iter().all(|s| !s.is_empty()),
        "--to names an agent, name@hamlet or mayor@hamlet"
    );
    let target = resolve(
        &state,
        parts[0],
        parts.get(1).copied().or(if parts[0] == MAYOR {
            Some(HOME)
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
    for deed in &state.deeds {
        eprintln!(
            "  /{}: {}",
            format!("{} {}", deed.name, deed.usage).trim_end(),
            deed.description
        );
    }
    if state.settings.unset {
        eprintln!(
            "Set up a provider with `hamlet login <provider> \
             [--key]`, then try a hello with `hamlet models try`."
        );
    }
    let mut session = Session {
        client,
        state,
        target,
        screen,
        interactive: std::io::stdin().is_terminal(),
        opens: HashSet::new(),
        prompted: false,
        last: None,
        ending: false,
        final_id: None,
        pending: HashSet::new(),
        numbers: HashMap::new(),
        pinned: HashSet::new(),
    };
    let chat = chat_ref(session.target.clone());
    session.open(RequestId::new(), chat).await?;
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            line = input.next_line(), if session.last.is_none() => {
                session.on_line(line?).await?;
            }
            message = session.client.next() => {
                if session.on_message(message?).await? {
                    break;
                }
            }
        }
    }
    session.screen.end()?;
    session.client.close().await
}

/// A terminal chat with one agent at a time.
struct Session {
    client: Client,
    state: State,
    target: Address,
    screen: Screen,
    interactive: bool,
    /// The opens whose failure ends the chat.
    opens: HashSet<RequestId>,
    prompted: bool,
    /// The open sent at the end of input, once it is sent.
    last: Option<RequestId>,
    /// Whether the open sent at the end of input was answered.
    ending: bool,
    /// The open that ends the chat once its transcript arrives.
    final_id: Option<RequestId>,
    /// The lines said and not yet answered.
    pending: HashSet<RequestId>,
    /// The current conversation's number, by agent.
    numbers: HashMap<String, i64>,
    /// The past conversations opened to finish their turns.
    pinned: HashSet<(String, i64)>,
}

impl Session {
    /// Opens `chat`, ending the chat if the server refuses.
    async fn open(
        &mut self,
        id: RequestId,
        chat: ChatRef,
    ) -> Result<()> {
        self.opens.insert(id.clone());
        self.client
            .send(ClientMessage::Open(Open {
                id,
                chat,
                before: None,
            }))
            .await
    }

    /// Takes a line the chief typed, or the end of input.
    async fn on_line(&mut self, line: Option<String>) -> Result<()> {
        let Some(line) = line else {
            let barrier = RequestId::new();
            self.open(barrier.clone(), chat_ref(self.target.clone()))
                .await?;
            self.last = Some(barrier);
            return Ok(());
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return prompt(&mut self.screen, self.interactive);
        }
        let (command, name) = trimmed
            .split_once(char::is_whitespace)
            .unwrap_or((trimmed, ""));
        if command != "/agent" && command != "/hamlet" {
            let id = RequestId::new();
            self.pending.insert(id.clone());
            let to = self.target.clone();
            return self
                .client
                .send(ClientMessage::Say(Say { id, to, text: line }))
                .await;
        }
        let name = name.trim();
        match switch(&self.state, &self.target, command, name) {
            Ok(next) => {
                if next != self.target {
                    self.target = next;
                    self.open(
                        RequestId::new(),
                        chat_ref(self.target.clone()),
                    )
                    .await?;
                }
                let whom = match command {
                    "/agent" => "agent",
                    _ => "mayor of",
                };
                eprintln!("  (talking to the {whom} {name})");
            }
            Err(error) => eprintln!("error: {error}"),
        }
        prompt(&mut self.screen, self.interactive)
    }

    /// Takes a server message. True once the chat is over.
    async fn on_message(
        &mut self,
        message: ServerMessage,
    ) -> Result<bool> {
        if let ServerMessage::Fault(fault) = &message
            && fault
                .to
                .as_ref()
                .is_some_and(|id| self.opens.contains(id))
        {
            bail!("{}", fault.text);
        }
        self.track(&message).await?;
        let chat = match &message {
            ServerMessage::Transcript(t) => Some(&t.chat),
            ServerMessage::Turn(t) => Some(&t.chat),
            _ => None,
        };
        let duplicate = chat.is_some_and(|chat| {
            chat.past.is_some()
                && chat.past
                    == self
                        .numbers
                        .get(&address_key(&chat.with))
                        .copied()
        });
        if !duplicate {
            self.screen.show(&message)?;
        }
        let waiting = self.interactive && self.last.is_none();
        if let ServerMessage::Transcript(transcript) = &message {
            if !self.prompted {
                self.prompted = true;
                prompt(&mut self.screen, waiting)?;
            }
            if self.last.is_some() && transcript.to == self.last {
                self.ending = true;
            }
        }
        if matches!(
            message,
            ServerMessage::Reply(_) | ServerMessage::Fault(_)
        ) && message.to().is_some_and(|id| self.pending.remove(id))
        {
            prompt(&mut self.screen, waiting)?;
        }
        if self.ending
            && self.pending.is_empty()
            && self.final_id.is_none()
        {
            let barrier = RequestId::new();
            self.open(barrier.clone(), chat_ref(self.target.clone()))
                .await?;
            self.final_id = Some(barrier);
        }
        Ok(matches!(
            message,
            ServerMessage::Transcript(transcript)
                if self.final_id.is_some() && transcript.to == self.final_id
        ))
    }

    /// Keeps the state current, and opens a past conversation whose
    /// turn a new one interrupted.
    async fn track(&mut self, message: &ServerMessage) -> Result<()> {
        match message {
            ServerMessage::State(next) => self.state = next.clone(),
            ServerMessage::Change(changes) => {
                for change in &changes.changes {
                    self.state.apply(change);
                }
            }
            ServerMessage::Transcript(transcript) => {
                if let Some(to) = &transcript.to {
                    self.opens.remove(to);
                }
                let key = address_key(&transcript.chat.with);
                if transcript.chat.past.is_none()
                    && let Some(number) = transcript.number
                    && let Some(previous) =
                        self.numbers.insert(key.clone(), number)
                    && previous != number
                    && self.pinned.insert((key, previous))
                {
                    let mut chat =
                        chat_ref(transcript.chat.with.clone());
                    chat.past = Some(previous);
                    self.open(RequestId::new(), chat).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }
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
            if name == MAYOR {
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
    resolve(state, MAYOR, Some(&hamlet.id))
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
                self.line(&format!("\n  ({})", item.text))?;
            }
            ItemKind::Error => {
                self.line(&format!("\nerror: {}", item.text))?;
            }
            ItemKind::Answered => {
                self.line(&format!("\n  chief:\n{}", item.text))?;
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
                self.line(&format!("\nerror: {}", fault.text))?;
            }
            ServerMessage::Reply(reply) if !reply.text.is_empty() => {
                self.line(&if reply.kind == ReplyKind::Error {
                    format!("\nerror: {}", reply.text)
                } else {
                    format!("\n  ({})", reply.text)
                })?;
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
                    t.items.clone(),
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
                let id =
                    |raw: &str| Self::item_id(&t.chat, t.number, raw);
                for op in &t.ops {
                    let applied = transcript::apply(
                        &mut items,
                        op,
                        Item::clone,
                    )
                    .is_ok();
                    match op {
                        Op::Add { item, replaces } => {
                            let mut item = item.clone();
                            item.id = id(&item.id);
                            let replaces =
                                replaces.as_deref().map(id);
                            self.add(&item, replaces.as_deref())?;
                        }
                        Op::Append { item, .. } => {
                            let Some(kept) = items
                                .iter()
                                .find(|kept| &kept.id == item)
                            else {
                                continue;
                            };
                            let id = id(item);
                            let printed = self
                                .streamed
                                .get(&id)
                                .map(|(n, _)| *n)
                                .unwrap_or(0);
                            self.stream(
                                &id,
                                kept.text
                                    .get(printed..)
                                    .unwrap_or_default(),
                            )?;
                        }
                        Op::Close { exit, .. }
                            if applied && *exit != 0 =>
                        {
                            self.line(&format!("  [exit {exit}]"))?;
                        }
                        Op::Status { working: false, .. } => {
                            self.end()?;
                        }
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
