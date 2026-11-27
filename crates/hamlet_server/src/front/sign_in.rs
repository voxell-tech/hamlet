//! The sign-ins the door's clients start, each answered and ended by
//! the connection that started it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::anyhow;
use hamlet_messages::address;
use hamlet_messages::agent::chat::{Login, LoginAnswer};
use hamlet_protocol::client::{
    self as protocol, LoginProgress, LoginReply, LoginStep,
    RequestId, ServerMessage,
};

use super::call::failure;
use super::host::HAND;
use super::route::After;
use super::{Asker, Door};
use crate::clients::check::reply;
use crate::society::refusal::Refusal;

impl Door {
    /// Takes a step of a sign-in: one its client starts, answers or
    /// cancels. Only the connection that started a sign-in may
    /// answer, cancel or start it again.
    pub(super) fn sign_in(
        &mut self,
        client: u64,
        login: protocol::Login,
    ) {
        let asker = self.ask(client);
        let sign_ins = self.sign_ins.clone();
        match login {
            protocol::Login::Start { id, start } => {
                let login = id.to_string();
                let held = {
                    let mut open = sign_ins.lock();
                    let held = open
                        .get(&login)
                        .is_some_and(|open| open.client != client);
                    if !held {
                        open.insert(
                            login.clone(),
                            SignIn {
                                client,
                                start: id.clone(),
                                provider: start.provider.clone(),
                                last: None,
                                asking: false,
                            },
                        );
                    }
                    held
                };
                if held {
                    asker.send(reply(&id, Err(no_sign_in())));
                    return;
                }
                let start = protocol::Login::Start { id, start };
                self.work.spawn(sign_ins.steps(asker, login, start));
            }
            protocol::Login::Reply { id, reply: answer } => {
                let refusal = {
                    let mut open = sign_ins.lock();
                    match open
                        .get_mut(&answer.login)
                        .filter(|open| open.client == client)
                    {
                        None => Some(no_sign_in()),
                        Some(open) if !open.asking => {
                            Some(Refusal::Usage {
                                words: "That sign-in is not waiting \
                                        for an answer."
                                    .into(),
                            })
                        }
                        Some(open) => {
                            open.asking = false;
                            None
                        }
                    }
                };
                if let Some(refusal) = refusal {
                    asker.send(reply(&id, Err(refusal)));
                    return;
                }
                self.credentials.remember(&answer.value);
                asker.send(reply(
                    &id,
                    Ok("Sign-in answer received.".to_owned()),
                ));
                let login = answer.login.clone();
                let answered =
                    protocol::Login::Reply { id, reply: answer };
                self.work
                    .spawn(sign_ins.steps(asker, login, answered));
            }
            protocol::Login::Cancel { id, cancel } => {
                let open = {
                    let mut open = sign_ins.lock();
                    open.get(&cancel)
                        .is_some_and(|open| open.client == client)
                        .then(|| open.remove(&cancel))
                        .flatten()
                };
                let Some(open) = open else {
                    asker.send(reply(&id, Err(no_sign_in())));
                    return;
                };
                asker.send(open.frame(
                    &cancel,
                    LoginStep::Failed {
                        failed: "Sign-in cancelled.".into(),
                    },
                ));
                asker.send(reply(
                    &id,
                    Ok("Sign-in cancelled.".to_owned()),
                ));
                self.work.spawn(async move {
                    let cancel =
                        protocol::Login::Cancel { cancel, id };
                    let _ = asker
                        .calls
                        .call(&address::Agent(HAND), &Login(cancel))
                        .await;
                });
            }
        }
    }
}

/// The sign-ins the door's clients started, by login: the id of the
/// `start` that began each.
#[derive(Clone, Default)]
pub(super) struct SignIns(Arc<Mutex<HashMap<String, SignIn>>>);

struct SignIn {
    client: u64,
    start: RequestId,
    provider: String,
    /// The step given last, which a step that finds nothing new
    /// gives again.
    last: Option<LoginStep>,
    /// Whether it waits for the chief's typed answer.
    asking: bool,
}

impl SignIn {
    /// `step` of sign-in `login`, as its client is sent it.
    fn frame(&self, login: &str, step: LoginStep) -> ServerMessage {
        ServerMessage::Login(LoginProgress {
            login: login.to_owned(),
            step,
            to: Some(self.start.clone()),
        })
    }
}

impl SignIns {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, SignIn>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Drops the sign-ins `client` started, whose connection closed,
    /// and gives their logins.
    pub(super) fn forget(&self, client: u64) -> Vec<String> {
        let mut open = self.lock();
        let gone = open
            .iter()
            .filter(|(_, open)| open.client == client)
            .map(|(login, _)| login.clone())
            .collect::<Vec<_>>();
        open.retain(|_, open| open.client != client);
        gone
    }

    /// Sends the Hand `first`, a step of sign-in `login`, then asks
    /// it for each next step and sends each new one to its client,
    /// until the sign-in waits for the chief, ends or is dropped.
    async fn steps(
        self,
        asker: Asker,
        login: String,
        first: protocol::Login,
    ) {
        let hand = address::Agent(HAND);
        let mut next = first;
        loop {
            let answer = asker.calls.call(&hand, &Login(next)).await;
            let step = match answer {
                Ok(LoginAnswer::Step { step, .. }) => step,
                Ok(LoginAnswer::Done { .. }) => {
                    let ended = self.lock().remove(&login);
                    if let Some(ended) = ended {
                        let done = LoginStep::Done {
                            done: ended.provider.clone(),
                        };
                        asker.after(After::Show).await;
                        asker.send(ended.frame(&login, done));
                    }
                    return;
                }
                Ok(LoginAnswer::Failed { words, .. }) => {
                    return self.end(&asker, &login, words);
                }
                Ok(LoginAnswer::NotTheHand) => {
                    let error =
                        anyhow!("agent {} is not the Hand", HAND.0);
                    return self.end(&asker, &login, failure(&error));
                }
                Err(error) => {
                    return self.end(&asker, &login, failure(&error));
                }
            };
            {
                let mut open = self.lock();
                let Some(going) = open
                    .get_mut(&login)
                    .filter(|going| going.client == asker.client)
                else {
                    return;
                };
                if going.last.as_ref() != Some(&step) {
                    asker.send(going.frame(&login, step.clone()));
                    going.last = Some(step.clone());
                }
                if let LoginStep::Prompt { .. } = step {
                    going.asking = true;
                    return;
                }
            }
            next = protocol::Login::Reply {
                id: RequestId::new(),
                reply: LoginReply {
                    login: login.clone(),
                    value: String::new(),
                },
            };
        }
    }

    /// Ends sign-in `login`, unless it ended, with its failure in
    /// `words`.
    fn end(&self, asker: &Asker, login: &str, words: String) {
        if let Some(ended) = self.lock().remove(login) {
            asker.send(
                ended.frame(
                    login,
                    LoginStep::Failed { failed: words },
                ),
            );
        }
    }
}

/// The refusal of a sign-in reply or cancel that is not the
/// connection's own open one.
pub(crate) fn no_sign_in() -> Refusal {
    Refusal::Usage {
        words: "No sign-in of yours is open.".into(),
    }
}
