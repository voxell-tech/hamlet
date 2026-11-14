//! The `workspace` actor, one per machine: it runs the agents'
//! commands and file changes under the sandbox, within each agent's
//! reach.

mod calls;
mod files;
mod pending;
pub mod reach;
mod run;
mod tools;
mod workers;

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{
    ActorConfig, ActorFactory, SqliteTransaction,
};
use hamlet_host::{Address, Caller, Host, HostRef};
use hamlet_messages::agent::{Finished, Outcome};
use hamlet_messages::realm::{NeedsChief, SecretList, Secrets};
use hamlet_messages::workspace::files::{
    EditFile, Find, ReadFile, Search, WriteFile,
};
use hamlet_messages::workspace::reach::{Reach, SecretsVersion};
use hamlet_messages::workspace::{
    AgentNumber, AnswerTaken, CallId, CallRefusal, CallerRefusal,
    Cancel, CancelAnswer, ChiefAnswer, CommandId, CommandStatus,
    Exit, Lapse, ListTools, Program, ReachAnswer, RealmRefusal, Run,
    RunAnswer, SetReach, ToolList, Which,
};
use hamlet_sandbox::{AllLeftovers, Sandbox};

use self::reach::{Gate, Purpose, RealmFolder};

pub const KIND: &str = "workspace";

/// The workspace's key.
pub const MACHINE: &str = "local";

/// The kind of the realm, the only caller that sets a reach.
pub const REALM: &str = "realm";

/// The realm's key.
pub const REALM_KEY: &str = "local";

/// The kind of an agent, keyed by its [`AgentNumber`].
pub const AGENT: &str = "agent";

/// The most lines and bytes of a command's output or a file handed
/// to an agent at once (C16).
const MOST_LINES: usize = 2000;
const MOST_BYTES: usize = 50 * 1024;

/// How the workspace answers, given by its host as a value of
/// [`hamlet_host::HostOptions`]. Without one, it waits 5 seconds.
pub struct Options {
    /// How long a call waits for its command before it answers
    /// `Running`.
    pub answer_within: Duration,
}

/// The workspace's kind. Its host gives it the [`Sandbox`] its
/// commands run in, as a value of [`hamlet_host::HostOptions`].
pub fn factory() -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<Workspace>(
        KIND,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry
        .into_factories()
        .remove(KIND)
        .expect("the registry holds the kind it registered")
}

struct Workspace {
    home: RealmFolder,
    sandbox: Sandbox,
    /// The realm's secrets, held in memory only: none after a
    /// start, so the first command fetches them.
    secrets: Mutex<Option<Held>>,
    /// What commands left running under Seatbelt, ended with the
    /// workspace.
    leftovers: AllLeftovers,
    /// How long a call waits for its command.
    answer_within: Duration,
    /// Each running command, until its end was heard.
    running: Mutex<HashMap<CommandId, run::Running>>,
    /// The paths a change is at work on.
    paths: files::Paths,
}

/// A list of the realm's secrets and its version.
struct Held {
    version: SecretsVersion,
    values: Arc<Vec<String>>,
}

#[async_trait::async_trait]
impl Actor for Workspace {
    type Input = ();
    type Actions = (
        SetReach,
        Run,
        CommandStatus,
        Cancel,
        ListTools,
        ReadFile,
        Search,
        Find,
        WriteFile,
        EditFile,
        ChiefAnswer,
    );
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        let host = HostRef::of(ctx.inner())?;
        let sandbox = *host
            .value::<Sandbox>()
            .context("the host gave the workspace no sandbox")?;
        // The commands of a host that stopped died with it, and no
        // one is told.
        ctx.sql()
            .exec(format!(
                "{};{};{};{}",
                reach::SCHEMA,
                calls::SCHEMA,
                pending::SCHEMA,
                run::CUT_OFF
            ))
            .await?;
        files::cut_off_changes(ctx).await?;
        let home = RealmFolder::new(host.folder())?;
        workers::sweep(ctx, &home).await?;
        Ok(Self {
            home,
            sandbox,
            secrets: Mutex::new(None),
            leftovers: AllLeftovers::default(),
            answer_within: host
                .value::<Options>()
                .map_or(run::ANSWER_WITHIN, |options| {
                    options.answer_within
                }),
            running: Mutex::new(HashMap::new()),
            paths: files::Paths::default(),
        })
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        self.leftovers.end(|_| true);
    }
}

impl Workspace {
    /// The placement of `agent`'s call of `purpose` under `reach`.
    /// A worker's scratch folder is made first when it is missing, as
    /// after a kill between its reach and its folder.
    fn placement(
        &self,
        agent: AgentNumber,
        reach: &Reach,
        purpose: Purpose,
    ) -> Result<reach::Placement> {
        let placement =
            reach::placement(&self.home, agent, reach, purpose)
                .context("the gate passed a call with no reach")?;
        if matches!(reach, Reach::Worker { .. }) {
            workers::make(&self.home, agent)
                .context("make a worker's scratch folder")?;
        }
        Ok(placement)
    }

    /// The policy `ready` runs under for `agent`, with the network
    /// when `network`: a script's from the agent's reach, a tool's
    /// from its manifest.
    fn program_placement(
        &self,
        agent: AgentNumber,
        reach: &Reach,
        ready: &tools::Ready<'_>,
        network: bool,
    ) -> Result<reach::Placement> {
        match ready {
            tools::Ready::Script { .. } => self.placement(
                agent,
                reach,
                Purpose::Command { network },
            ),
            tools::Ready::Tool { tool, .. } => {
                Ok(tool.placement(network))
            }
        }
    }

    /// The version of the secrets held, and the list.
    fn held(&self) -> (Option<SecretsVersion>, Arc<Vec<String>>) {
        let held = self
            .secrets
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        held.as_ref().map_or_else(
            || (None, Arc::default()),
            |held| (Some(held.version), held.values.clone()),
        )
    }

    /// What `weigh` comes to under a list of the realm's secrets as
    /// new as the reach in force asks for. When it asks for a newer
    /// list, the list is fetched from the realm, once, and `weigh`
    /// runs again.
    async fn weighed<T, W>(
        &self,
        ctx: &Ctx<Self>,
        mut weigh: impl FnMut() -> W,
    ) -> Result<T>
    where
        W: Future<Output = Result<Weighing<T>>>,
    {
        if let Weighing::Done(done) = weigh().await? {
            return Ok(done);
        }
        self.fetch_secrets(ctx).await?;
        match weigh().await? {
            Weighing::Done(done) => Ok(done),
            Weighing::NeedsSecrets(version) => bail!(
                "the realm's secrets are older than version {}",
                version.0
            ),
        }
    }

    /// Calls the realm for its secrets, and holds them when they are
    /// newer than the ones held.
    async fn fetch_secrets(&self, ctx: &Ctx<Self>) -> Result<()> {
        let answer =
            call_actor(ctx, Address::new(REALM, REALM_KEY), &Secrets)
                .await
                .context("fetch the realm's secrets")?;
        let list: SecretList =
            ciborium::from_reader(answer.as_slice())
                .context("decode the realm's secrets")?;
        let mut held = self
            .secrets
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if held
            .as_ref()
            .is_none_or(|held| held.version < list.version)
        {
            *held = Some(Held {
                version: list.version,
                values: Arc::new(list.values),
            });
        }
        Ok(())
    }
}

/// What weighing a call came to, or the version of the realm's
/// secrets it needs first.
enum Weighing<T> {
    Done(T),
    NeedsSecrets(SecretsVersion),
}

/// What the gate let a command do, once its row is open.
struct Opened {
    running: run::Running,
    placement: reach::Placement,
    exec: (run::Exec, Duration),
    secrets: Arc<Vec<String>>,
}

impl Handler<ListTools> for Workspace {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: ListTools,
    ) -> Result<ToolList> {
        Ok(tools::list(&self.home))
    }
}

impl Handler<Run> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        run: Run,
    ) -> Result<RunAnswer> {
        let agent = match calling_agent(&ctx, run.agent, Run::NAME)? {
            Ok(agent) => agent,
            Err(refusal) => return Ok(RunAnswer::Refused(refusal)),
        };
        if let Err(why) = checked(&run) {
            return Ok(RunAnswer::Refused(CallRefusal::Invalid(
                why.to_string(),
            )));
        }
        let request_hash = calls::request_hash(&run)?;
        // Read before the row is opened: past its commit, nothing
        // returns before the command starts.
        let leftovers = self.leftovers.of(i64::try_from(agent.0)?);
        let (this, ctx, run) = (&*self, &ctx, &run);
        let request_hash = &request_hash;
        let decision = self
            .weighed(ctx, move || async move {
                let tx = ctx.sql().begin_transaction(None).await?;
                match this.decide(&tx, agent, run, request_hash).await
                {
                    Ok(Weighing::Done(Decision::Open(opened))) => {
                        if let Err(error) = tx.commit().await {
                            this.ran(opened.running.command);
                            return Err(error);
                        }
                        Ok(Weighing::Done(Decision::Open(opened)))
                    }
                    Ok(Weighing::Done(Decision::AskChief(
                        notice,
                    ))) => {
                        tx.commit().await?;
                        Ok(Weighing::Done(Decision::AskChief(notice)))
                    }
                    Ok(other) => {
                        tx.rollback().await?;
                        Ok(other)
                    }
                    Err(error) => {
                        tx.rollback().await?;
                        Err(error)
                    }
                }
            })
            .await?;
        let opened = match decision {
            Decision::Open(opened) => opened,
            Decision::Answer(answer) => return Ok(answer),
            Decision::Wait(command, waiters) => {
                return Ok(waiters.answer(command).await);
            }
            Decision::AskChief(notice) => {
                let needs = notice.needs.clone();
                self.ask_chief(ctx, notice).await;
                return Ok(RunAnswer::WaitsForChief(needs));
            }
        };
        let running = opened.running;
        let spec = self.spec(
            &running,
            opened.exec,
            opened.placement,
            opened.secrets,
            leftovers,
        );
        self.clone().start(ctx, &running, spec);
        Ok(running.waiters.answer(running.command).await)
    }
}

impl Workspace {
    /// The command `running`, which starts `exec` within its time
    /// limit, as its thread runs it.
    fn spec(
        &self,
        running: &run::Running,
        (exec, timeout): (run::Exec, Duration),
        placement: reach::Placement,
        secrets: Arc<Vec<String>>,
        leftovers: hamlet_sandbox::Leftovers,
    ) -> run::Spec {
        run::Spec {
            command: running.command,
            exec,
            cwd: placement.cwd,
            policy: placement.policy,
            sandbox: self.sandbox,
            timeout,
            secrets,
            leftovers,
            output: self
                .home
                .spill(running.agent)
                .join(format!("{}.log", running.command.0)),
            cancel: running.cancel.clone(),
        }
    }

    /// Runs `run`, whose `wait` a chief allowed, when the reach in
    /// force still lets it. The agent is called with its end.
    async fn grant_run(
        self: Arc<Self>,
        ctx: &Ctx<Self>,
        wait: &pending::Wait,
        run: &Run,
    ) -> Result<AnswerTaken> {
        let agent = wait.agent;
        let leftovers = self.leftovers.of(i64::try_from(agent.0)?);
        let ready = match tools::Ready::of(&self.home, run) {
            Ok(ready) => ready,
            Err(refusal) => {
                return self.not_run(ctx, wait, refusal).await;
            }
        };
        let (reach, network, secrets) = match self
            .recheck(ctx, wait, &ready.op(&run.conversation))
            .await?
        {
            pending::Rechecked::Pass {
                reach,
                network,
                secrets,
            } => (reach, network, secrets),
            pending::Rechecked::Refused(refusal) => {
                return self.not_run(ctx, wait, refusal).await;
            }
        };
        let placement =
            self.program_placement(agent, &reach, &ready, network)?;
        let tx = ctx.sql().begin_transaction(None).await?;
        let settled =
            pending::settle(&tx, wait, "running", None, None).await;
        let command = match settled {
            Ok(Some(id)) => CommandId(u64::try_from(id)?),
            Ok(None) => {
                tx.rollback().await?;
                return Ok(AnswerTaken::NoWait);
            }
            Err(error) => {
                tx.rollback().await?;
                return Err(error);
            }
        };
        // No call waits for it: the agent hears its end.
        let running = run::Running::new(
            command,
            agent,
            run.call.clone(),
            run.conversation.clone(),
            run::Waiters::given_up(),
        );
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(command, running.clone());
        if let Err(error) = tx.commit().await {
            self.ran(command);
            return Err(error);
        }
        let spec = self.spec(
            &running,
            ready.exec(),
            placement,
            secrets,
            leftovers,
        );
        self.clone().start(ctx, &running, spec);
        Ok(AnswerTaken::Runs)
    }
}

impl Handler<CommandStatus> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: CommandStatus,
    ) -> Result<RunAnswer> {
        let agent = match calling_agent(
            &ctx,
            asked.agent,
            CommandStatus::NAME,
        )? {
            Ok(agent) => agent,
            Err(refusal) => return Ok(RunAnswer::Refused(refusal)),
        };
        let row = read_only(&ctx, async |tx| {
            run::status(tx, agent, asked.command).await
        })
        .await?;
        Ok(match row {
            None => RunAnswer::Refused(CallRefusal::NoSuchCommand(
                asked.command,
            )),
            Some(row) if self.waiters(row.command).is_some() => {
                RunAnswer::Running {
                    command: row.command,
                }
            }
            Some(row) => answer_of(row),
        })
    }
}

impl Workspace {
    /// Starts the command of `spec`, whose row was just committed,
    /// and a task that records its end and has it heard: by the calls
    /// waiting for it, else by its agent, called with `Finished`.
    /// Nothing between the commit and this awaits, so a row is never
    /// left running with no process while the host runs.
    fn start(
        self: Arc<Self>,
        ctx: &Ctx<Self>,
        running: &run::Running,
        spec: run::Spec,
    ) {
        let (command, agent) = (running.command, running.agent);
        let call = running.call.clone();
        let abort = ctx.abort_signal();
        let ended = run::start(spec, abort.clone());
        // The actor stays awake while a command runs, so its end is
        // recorded and heard.
        let awake = ctx.keep_awake_region();
        let task_ctx = ctx.clone();
        ctx.register_task(async move {
            let _awake = awake;
            let ctx = task_ctx;
            let end = match ended.await {
                Ok(end) => end,
                // The host stops: the next start marks the row cut
                // off.
                Err(_) if abort.is_cancelled() => {
                    self.ran(command);
                    return;
                }
                // Its thread panicked.
                Err(_) => run::without_output(
                    command,
                    Exit::Failed(
                        "the command's thread ended before it".into(),
                    ),
                ),
            };
            if let Err(error) =
                transact(&ctx, async |tx| run::end(tx, &end).await)
                    .await
            {
                tracing::warn!(
                    ?error,
                    command = command.0,
                    "record a command's end"
                );
            }
            let Some(waiters) = self.waiters(command) else {
                return;
            };
            let unheard = waiters.ended(end);
            // Stopped before the agent is told, so a cancel need not
            // wait for that call.
            self.ran(command);
            if let Some(end) = unheard {
                tell(&ctx, agent, call, Outcome::Ran(end)).await;
            }
        });
    }

    fn waiters(
        &self,
        command: CommandId,
    ) -> Option<Arc<run::Waiters>> {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&command)
            .map(|running| running.waiters.clone())
    }

    /// Forgets `command`, whose end is recorded or never will be,
    /// and lets go whoever waits for it to stop.
    fn ran(&self, command: CommandId) {
        let running = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&command);
        if let Some(running) = running {
            running.stopped.cancel();
        }
    }

    /// Ends `agent`'s running commands that `which` names, as
    /// cancelled, and waits until each has stopped. Returns their
    /// ids, none when none ran.
    async fn cancel(
        &self,
        ctx: &Ctx<Self>,
        agent: AgentNumber,
        which: &Which,
    ) -> Vec<CommandId> {
        let named: Vec<run::Running> = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .filter(|running| running.is(agent, which))
            .cloned()
            .collect();
        for running in &named {
            running.cancel.cancel();
        }
        let abort = ctx.abort_signal();
        for running in &named {
            tokio::select! {
                () = running.stopped.cancelled() => {}
                () = abort.cancelled() => {}
            }
        }
        let mut commands: Vec<CommandId> =
            named.iter().map(|running| running.command).collect();
        commands.sort_by_key(|command| command.0);
        commands
    }
}

impl Handler<Cancel> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        cancel: Cancel,
    ) -> Result<CancelAnswer> {
        let caller = caller(&ctx)?;
        if !is_the_realm(caller.as_ref())
            && let Err(refusal) =
                agent_of(caller.as_ref(), cancel.agent)
        {
            refused(caller.as_ref(), Cancel::NAME, &refusal);
            return Ok(CancelAnswer::Refused(refusal));
        }
        Ok(CancelAnswer::Cancelled(
            self.cancel(&ctx, cancel.agent, &cancel.which).await,
        ))
    }
}

async fn transact<T>(
    ctx: &Ctx<Workspace>,
    work: impl AsyncFnOnce(&SqliteTransaction) -> Result<T>,
) -> Result<T> {
    let Ok(done) = transact_if(ctx, async |tx| {
        work(tx).await.map(Ok::<T, Infallible>)
    })
    .await?;
    Ok(done)
}

async fn transact_if<T, E>(
    ctx: &Ctx<Workspace>,
    work: impl AsyncFnOnce(&SqliteTransaction) -> Result<Result<T, E>>,
) -> Result<Result<T, E>> {
    let tx = ctx.sql().begin_transaction(None).await?;
    match work(&tx).await {
        Ok(Ok(done)) => {
            tx.commit().await?;
            Ok(Ok(done))
        }
        other => {
            tx.rollback().await?;
            other
        }
    }
}

async fn read_only<T>(
    ctx: &Ctx<Workspace>,
    work: impl AsyncFnOnce(&SqliteTransaction) -> Result<T>,
) -> Result<T> {
    let tx = ctx.sql().begin_transaction(None).await?;
    let done = work(&tx).await;
    tx.rollback().await?;
    done
}

async fn call_actor<A: Action>(
    ctx: &Ctx<Workspace>,
    to: Address,
    action: &A,
) -> Result<Vec<u8>> {
    let args = hamlet_actor::action::encode_positional(action)?;
    Host::call_from(
        ctx.inner(),
        &to,
        A::NAME,
        args,
        Duration::from_secs(10),
    )
    .await
}

/// Calls `agent` with how its call `call` ended, once. A failed
/// call, as to an agent that no longer exists, is logged and dropped.
async fn tell(
    ctx: &Ctx<Workspace>,
    agent: AgentNumber,
    call: CallId,
    outcome: Outcome,
) {
    let id = call.0.clone();
    let to = Address::new(AGENT, agent.0.to_string());
    if let Err(error) =
        call_actor(ctx, to, &Finished { call, outcome }).await
    {
        tracing::warn!(
            ?error,
            agent = agent.0,
            call = id,
            "dropped the end of a call the agent was not told"
        );
    }
}

/// What a [`Run`]'s transaction comes to.
enum Decision {
    Open(Opened),
    Answer(RunAnswer),
    /// A repeat of a call whose command runs: it hears what the first
    /// call hears.
    Wait(CommandId, Arc<run::Waiters>),
    /// The call waits for a chief: commit, then tell the realm.
    AskChief(NeedsChief),
}

impl Workspace {
    /// Answers a repeat of `run` from its row, or passes it through
    /// the gate and opens its row, in `tx`.
    async fn decide(
        &self,
        tx: &SqliteTransaction,
        agent: AgentNumber,
        run: &Run,
        request_hash: &[u8],
    ) -> Result<Weighing<Decision>> {
        if let Some(call) = calls::find(tx, agent, &run.call).await? {
            if !call.is(run::OP, request_hash) {
                return Ok(Weighing::Done(Decision::Answer(
                    RunAnswer::Refused(CallRefusal::CallReused(
                        run.call.clone(),
                    )),
                )));
            }
            let row = run::row_of(call)?;
            if let Some(waiters) = self.waiters(row.command) {
                return Ok(Weighing::Done(Decision::Wait(
                    row.command,
                    waiters,
                )));
            }
            return Ok(Weighing::Done(Decision::Answer(answer_of(
                row,
            ))));
        }
        let ready = match tools::Ready::of(&self.home, run) {
            Ok(ready) => ready,
            Err(refusal) => {
                return Ok(Weighing::Done(Decision::Answer(
                    RunAnswer::Refused(refusal),
                )));
            }
        };
        let in_force = reach::in_force(tx, agent).await?;
        let reach_version =
            in_force.as_ref().map(|(version, _)| *version);
        let reach = in_force.map(|(_, reach)| reach);
        let (held, secrets) = self.held();
        let op = ready.op(&run.conversation);
        let network = match reach::gate(
            &self.home,
            agent,
            reach.as_ref(),
            held,
            &op,
        ) {
            Gate::Run { network } => network,
            Gate::Refuse(refusal) => {
                return Ok(Weighing::Done(Decision::Answer(
                    RunAnswer::Refused(CallRefusal::Reach(refusal)),
                )));
            }
            Gate::NeedsChief(needs) => {
                let opening = pending::Opening {
                    agent,
                    op: run::OP,
                    request_hash: request_hash.to_vec(),
                    needs,
                    request: pending::Pending::Run(run.clone()),
                    reach_version: reach_version
                        .context("a chief is asked with no reach")?,
                    base_hash: None,
                    words: tools::words(run),
                    change: None,
                };
                return Ok(Weighing::Done(
                    match pending::open(tx, &secrets, opening).await?
                    {
                        Ok(notice) => Decision::AskChief(notice),
                        Err(refusal) => Decision::Answer(
                            RunAnswer::Refused(refusal),
                        ),
                    },
                ));
            }
            Gate::NeedsSecrets(version) => {
                return Ok(Weighing::NeedsSecrets(version));
            }
        };
        let placement = self.program_placement(
            agent,
            reach
                .as_ref()
                .context("the gate passed a command with no reach")?,
            &ready,
            network,
        )?;
        let command =
            run::open(tx, agent, &run.call, request_hash.to_vec())
                .await?;
        // Before the commit, so a repeat that finds the row finds who
        // waits for it, and a reach that ends after the commit finds
        // the command to cancel.
        let running = run::Running::new(
            command,
            agent,
            run.call.clone(),
            run.conversation.clone(),
            run::Waiters::new(self.answer_within),
        );
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(command, running.clone());
        Ok(Weighing::Done(Decision::Open(Opened {
            running,
            placement,
            exec: ready.exec(),
            secrets,
        })))
    }
}

/// The answer a command's row gives.
fn answer_of(row: run::Row) -> RunAnswer {
    match row.state {
        run::State::Running => RunAnswer::Running {
            command: row.command,
        },
        run::State::Ended(end) => RunAnswer::Done(end),
        run::State::CutOff => RunAnswer::Done(run::without_output(
            row.command,
            Exit::CutOff,
        )),
        run::State::Waiting(needs) => RunAnswer::WaitsForChief(needs),
        run::State::Refused(refusal) => RunAnswer::Refused(refusal),
    }
}

/// Why a call is not a request the workspace takes, in the words of
/// the server's tools.
#[derive(Debug, thiserror::Error)]
enum Invalid {
    #[error(
        "Invalid call id: {0:?} is not 1 to 128 of A-Z, a-z, 0-9, \
         '.', '_', ':' and '-'"
    )]
    CallId(String),
    #[error("Invalid command: longer than 1 MiB")]
    LongCommand,
    #[error("Invalid timeout: must be a positive number of seconds")]
    ZeroTimeout,
    #[error("Invalid arguments: longer than 1 MiB")]
    LongArguments,
    #[error("{0} is not an absolute path")]
    NotAbsolute(String),
}

/// Refuses `run` where it is not a request the workspace takes.
fn checked(run: &Run) -> Result<(), Invalid> {
    checked_call(&run.call)?;
    match &run.program {
        Program::Script { script, .. }
            if script.len() > 1024 * 1024 =>
        {
            Err(Invalid::LongCommand)
        }
        Program::Script {
            timeout_ms: Some(0),
            ..
        } => Err(Invalid::ZeroTimeout),
        Program::Tool { arguments, .. }
            if arguments.len() > 1024 * 1024 =>
        {
            Err(Invalid::LongArguments)
        }
        Program::Script { .. } | Program::Tool { .. } => Ok(()),
    }
}

/// Refuses `call` where it is not an id the workspace takes.
fn checked_call(call: &CallId) -> Result<(), Invalid> {
    let call = &call.0;
    if call.is_empty()
        || call.len() > 128
        || !call.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || b"._:-".contains(&byte)
        })
    {
        return Err(Invalid::CallId(call.clone()));
    }
    Ok(())
}

impl Handler<SetReach> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        set: SetReach,
    ) -> Result<ReachAnswer> {
        if let Err(refusal) = realm_calling(&ctx, SetReach::NAME)? {
            return Ok(ReachAnswer::Refused(refusal));
        }
        let (in_force, now, closed) = transact(&ctx, async |tx| {
            let in_force = reach::apply(tx, &set).await?;
            // The reach in force, not the call's: an ended reach
            // repeated under its version with another body stays
            // ended.
            let now = reach::read(tx, set.agent).await?;
            let closed = if now == Some(Reach::Ended) {
                pending::close(tx, set.agent, &LAPSED).await?
            } else {
                Vec::new()
            };
            Ok((in_force, now, closed))
        })
        .await?;
        match now {
            Some(Reach::Ended) => {
                self.end_agent(&ctx, set.agent, closed).await?;
            }
            Some(Reach::Worker { .. }) => {
                if let Err(error) =
                    workers::make(&self.home, set.agent)
                {
                    // Its first call makes it again.
                    tracing::warn!(
                        ?error,
                        worker = set.agent.0,
                        "make a worker's scratch folder"
                    );
                }
            }
            Some(Reach::Agent(_)) | None => {}
        }
        Ok(ReachAnswer::InForce(in_force))
    }
}

/// Why the waits of an agent whose reach ended do not run.
const LAPSED: CallRefusal = CallRefusal::Lapsed(Lapse::Ended);

impl Workspace {
    /// Ends what `agent`, whose reach ended, has running: its
    /// commands, what they left under Seatbelt, and then its scratch
    /// folder when it was a worker. Its waits in `closed` ended with
    /// the reach, and the agent is told of each apart from this call,
    /// which a gone agent would hold up.
    async fn end_agent(
        &self,
        ctx: &Ctx<Self>,
        agent: AgentNumber,
        closed: Vec<CallId>,
    ) -> Result<()> {
        self.cancel(ctx, agent, &Which::All).await;
        let owner = i64::try_from(agent.0)?;
        self.leftovers.end(|of| of == owner);
        workers::remove(&self.home, agent).await;
        let task_ctx = ctx.clone();
        ctx.register_task(async move {
            for call in closed {
                tell(&task_ctx, agent, call, Outcome::NotRun(LAPSED))
                    .await;
            }
        });
        Ok(())
    }
}

/// Who made the call the handler runs for, as the host stamped it.
fn caller(ctx: &Ctx<Workspace>) -> Result<Option<Caller>> {
    ctx.conn().context("the call has no connection")?.params()
}

fn is_the_realm(caller: Option<&Caller>) -> bool {
    matches!(caller, Some(Caller::Actor(from)) if from.kind == REALM)
}

fn realm_calling(
    ctx: &Ctx<Workspace>,
    action: &str,
) -> Result<Result<(), RealmRefusal>> {
    let caller = caller(ctx)?;
    if is_the_realm(caller.as_ref()) {
        return Ok(Ok(()));
    }
    let refusal = RealmRefusal::NotTheRealm;
    refused(caller.as_ref(), action, &refusal);
    Ok(Err(refusal))
}

fn calling_agent(
    ctx: &Ctx<Workspace>,
    named: AgentNumber,
    action: &str,
) -> Result<Result<AgentNumber, CallRefusal>> {
    let caller = caller(ctx)?;
    Ok(agent_of(caller.as_ref(), named).map_err(|refusal| {
        refused(caller.as_ref(), action, &refusal);
        CallRefusal::Caller(refusal)
    }))
}

/// The agent a call of an agent's tool is for: the stamped caller,
/// which must be an agent and the one the request names.
pub fn agent_of(
    caller: Option<&Caller>,
    named: AgentNumber,
) -> Result<AgentNumber, CallerRefusal> {
    let Some(Caller::Actor(from)) = caller else {
        return Err(CallerRefusal::NotAnAgent);
    };
    if from.kind != AGENT {
        return Err(CallerRefusal::NotAnAgent);
    }
    let caller = from
        .key
        .parse()
        .map(AgentNumber)
        .map_err(|_| CallerRefusal::NotAnAgent)?;
    if caller == named {
        Ok(caller)
    } else {
        Err(CallerRefusal::NotTheCaller { named, caller })
    }
}

/// Logs that `caller` was refused `action`, and why. A refusal
/// leaves no row, so a caller refused again and again cannot fill
/// the workspace's file.
fn refused(
    caller: Option<&Caller>,
    action: &str,
    why: &impl std::fmt::Display,
) {
    let caller = match caller {
        Some(Caller::Actor(from)) => {
            format!("{}/{}", from.kind, from.key)
        }
        Some(Caller::Client) => "client".to_owned(),
        None => "none".to_owned(),
    };
    tracing::warn!(%caller, action, %why, "refused a call");
}

#[cfg(test)]
mod tests {
    use hamlet_host::Address;

    use super::*;

    fn actor(kind: &str, key: &str) -> Option<Caller> {
        Some(Caller::Actor(Address::new(kind, key)))
    }

    #[test]
    fn a_call_is_for_the_agent_that_made_it() {
        assert_eq!(
            agent_of(actor(AGENT, "7").as_ref(), AgentNumber(7)),
            Ok(AgentNumber(7))
        );
    }

    #[test]
    fn an_agent_that_names_another_is_refused() {
        assert_eq!(
            agent_of(actor(AGENT, "8").as_ref(), AgentNumber(7)),
            Err(CallerRefusal::NotTheCaller {
                named: AgentNumber(7),
                caller: AgentNumber(8),
            })
        );
    }

    #[test]
    fn only_an_agent_calls_for_its_tools() {
        for caller in [
            actor(REALM, "7"),
            actor(AGENT, "hand"),
            Some(Caller::Client),
            None,
        ] {
            assert_eq!(
                agent_of(caller.as_ref(), AgentNumber(7)),
                Err(CallerRefusal::NotAnAgent),
                "{caller:?}"
            );
        }
    }
}
