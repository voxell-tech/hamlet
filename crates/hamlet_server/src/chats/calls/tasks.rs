//! The tool calls and prompt preparations running for runtimes, each
//! a task answered when it ends.

use std::collections::HashMap;
use std::future::Future;

use serde_json::Value;
use tokio::task::{AbortHandle, Id, JoinError, JoinSet};

use super::Preparation;
use crate::harness::{BindingId, Call, ReplyAddress};
use crate::{OperationId, tools};

/// The tasks running for runtimes' calls.
#[derive(Default)]
pub(crate) struct Tasks {
    tools: JoinSet<tools::Outcome>,
    running: HashMap<OperationId, Running>,
    preparing: JoinSet<Prepared>,
    preparations: HashMap<Id, Preparing>,
}

/// A tool call running, and the calls of runtimes it answers.
struct Running {
    addresses: Vec<ReplyAddress>,
    binding: BindingId,
    abort: AbortHandle,
}

struct Preparing {
    binding: BindingId,
    address: ReplyAddress,
    abort: AbortHandle,
}

/// A prompt or a preview prepared for the call `call`, whose
/// preparation began when the state it reads had `signature`.
pub(crate) struct Prepared {
    pub binding: BindingId,
    pub address: ReplyAddress,
    pub call: Call,
    pub signature: Value,
    pub result: Preparation,
}

/// A task that ended.
#[expect(
    clippy::large_enum_variant,
    reason = "each is moved to its answer once, never kept"
)]
pub(crate) enum Finished {
    /// A tool call ran, with the calls it answers.
    Ran {
        id: OperationId,
        addresses: Vec<ReplyAddress>,
        outcome: tools::Outcome,
    },
    Prepared(Prepared),
    /// A preparation failed, and its call is answered with a refusal.
    Unprepared {
        address: ReplyAddress,
    },
}

/// The outcome of [`Tasks::join`].
#[derive(Debug, PartialEq)]
pub(crate) enum Join {
    /// The tool call runs, and answers the call joined too.
    Joined,
    /// No such tool call runs.
    NotRunning,
}

impl Tasks {
    /// Runs the tool call `id` of `binding`, answering `address` when
    /// it ends.
    pub fn run(
        &mut self,
        id: OperationId,
        binding: BindingId,
        address: ReplyAddress,
        task: impl Future<Output = tools::Outcome> + Send + 'static,
    ) {
        let abort = self.tools.spawn(task);
        self.running.insert(
            id,
            Running {
                addresses: vec![address],
                binding,
                abort,
            },
        );
    }

    /// Lets `address` be answered too when the tool call `id` ends,
    /// if `id` runs.
    pub fn join(
        &mut self,
        id: &OperationId,
        address: &ReplyAddress,
    ) -> Join {
        match self.running.get_mut(id) {
            Some(running) => {
                running.addresses.push(address.clone());
                Join::Joined
            }
            None => Join::NotRunning,
        }
    }

    /// Prepares the call of `binding` that came in at `address`.
    pub fn prepare(
        &mut self,
        binding: BindingId,
        address: ReplyAddress,
        task: impl Future<Output = Prepared> + Send + 'static,
    ) {
        let abort = self.preparing.spawn(task);
        self.preparations.insert(
            abort.id(),
            Preparing {
                binding,
                address,
                abort,
            },
        );
    }

    /// Stops the tasks of `binding`.
    pub fn stop(&mut self, binding: BindingId) {
        self.preparations.retain(|_, preparing| {
            let keep = preparing.binding != binding;
            if !keep {
                preparing.abort.abort();
            }
            keep
        });
        self.running.retain(|_, running| {
            let keep = running.binding != binding;
            if !keep {
                running.abort.abort();
            }
            keep
        });
    }

    /// Stops every task.
    pub fn stop_all(&mut self) {
        self.tools.abort_all();
        self.running.clear();
        self.preparing.abort_all();
        self.preparations.clear();
    }

    /// The next task to end that is still wanted; `None` when none
    /// runs. A task that panicked ends as a failure.
    pub async fn next(&mut self) -> Option<Finished> {
        loop {
            let ended = tokio::select! {
                Some(ran) = self.tools.join_next_with_id() => {
                    self.ran(ran)
                }
                Some(prepared) = self.preparing.join_next_with_id() => {
                    self.prepared(prepared)
                }
                else => return None,
            };
            if let Some(ended) = ended {
                return Some(ended);
            }
        }
    }

    fn ran(
        &mut self,
        ran: Result<(Id, tools::Outcome), JoinError>,
    ) -> Option<Finished> {
        let (task, outcome) = match ran {
            Ok(ran) => ran,
            Err(error) if error.is_cancelled() => return None,
            Err(error) => (error.id(), failure(&error)),
        };
        let id = self
            .running
            .iter()
            .find(|(_, running)| running.abort.id() == task)
            .map(|(id, _)| id.clone())?;
        let running = self.running.remove(&id)?;
        Some(Finished::Ran {
            id,
            addresses: running.addresses,
            outcome,
        })
    }

    fn prepared(
        &mut self,
        prepared: Result<(Id, Prepared), JoinError>,
    ) -> Option<Finished> {
        match prepared {
            Ok((task, prepared)) => {
                self.preparations.remove(&task)?;
                Some(Finished::Prepared(prepared))
            }
            Err(error) if error.is_cancelled() => None,
            Err(error) => {
                let preparing =
                    self.preparations.remove(&error.id())?;
                Some(Finished::Unprepared {
                    address: preparing.address,
                })
            }
        }
    }
}

/// The outcome of a tool call whose task panicked.
fn failure(error: &JoinError) -> tools::Outcome {
    tools::Outcome::thrown(
        "tool_failed",
        format!("The tool failed inside the server: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeInstance;

    #[tokio::test]
    async fn a_tool_that_panics_still_answers_its_calls() {
        let mut tasks = Tasks::default();
        let id = OperationId {
            runtime: RuntimeInstance::new("pi-1").unwrap(),
            key: "7".into(),
        };
        let address = ReplyAddress::test();
        tasks.run(id.clone(), BindingId(1), address.clone(), async {
            panic!("a tool's bug")
        });
        let Some(Finished::Ran {
            id: ended,
            addresses,
            outcome,
        }) = tasks.next().await
        else {
            panic!("the panicked tool call was never answered");
        };
        assert_eq!(ended, id);
        assert_eq!(addresses, vec![address.clone()]);
        assert!(outcome.error);
        assert_eq!(
            tasks.join(&id, &address),
            Join::NotRunning,
            "a panicked call still counts as running"
        );
        assert!(tasks.next().await.is_none());
    }
}
