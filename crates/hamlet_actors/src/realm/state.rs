//! The world as the client protocol shows it, in a chief's words.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use hamlet_actor::BindParam;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::budget::{Payer, money};
use hamlet_messages::realm::client::ReadState;
use hamlet_messages::realm::documents::Newcomers;
use hamlet_messages::realm::oversight::Permission;
use hamlet_messages::realm::world::{
    AgentSeen, Lasting, Seen, Starter, Status, WaitKind, WaitSeen,
};
use hamlet_messages::realm::{AgentName, HowEnded, Left};
use hamlet_messages::sender::{Client, Hand};
use hamlet_messages::workspace::reach::Need;
use hamlet_messages::workspace::{AgentNumber, Numbered};
use hamlet_protocol::client::{
    self as protocol, WaitDanger, WaitDetail,
};

use super::act::Look;
use super::change::{self, Column as _, Rows, number};
use super::chiefs::limits;
use super::tree::HAND_PROFILE;
use super::waits::Kind;
use super::{
    Realm, budget, chiefs, letters, profiles, record, settings,
};

mod change_shown;
mod hamlets;

use hamlets::{Period, Place, Spending, place};

/// What the realm shows a client beside the world, read at each
/// commit. What moves with the clock alone, a budget's period and a
/// road's wear, is reckoned as a client reads it.
#[derive(Clone, Default)]
pub(super) struct Shown {
    agents: Vec<protocol::Agent>,
    /// Each agent's spending in its payer's period.
    spent: BTreeMap<AgentNumber, Spending>,
    hamlets: Vec<Place>,
    closed: Vec<protocol::ClosedHamlet>,
    scholars: Vec<protocol::Scholar>,
    grants: Vec<protocol::Grant>,
    waits: Vec<protocol::Wait>,
    limits: Option<protocol::RealmLimits>,
    settings: Option<protocol::SettingsView>,
}

impl Look<Client> for ReadState {
    type Output = protocol::State;

    fn look(
        &self,
        _by: &Client,
        realm: &Realm,
    ) -> Result<protocol::State> {
        let kept = realm.world.current();
        let shown = &kept.shown;
        let now = now();
        let agents = shown
            .agents
            .iter()
            .map(|agent| {
                let number = AgentNumber(
                    u32::try_from(agent.id.0).unwrap_or(u32::MAX),
                );
                let spent = shown
                    .spent
                    .get(&number)
                    .copied()
                    .unwrap_or_default()
                    .at(now);
                protocol::Agent {
                    busy: agent.left.is_none()
                        && realm.lines.busy(number),
                    spent: protocol::AgentSpent {
                        cost: money(spent.micros),
                        micros: count(spent.micros),
                        ms: count(spent.ms),
                        tokens: count(spent.tokens),
                    },
                    ..agent.clone()
                }
            })
            .collect();
        let mut settings =
            shown.settings.clone().unwrap_or_else(unset);
        for endpoint in &mut settings.endpoints {
            endpoint.key = realm.secrets.keeps(&endpoint.name);
        }
        settings.workspace.url = realm.secrets.workspace_url();
        let state = protocol::State {
            agents,
            chief: String::new(),
            closed: shown.closed.clone(),
            confinement: protocol::Confinement::Insecure,
            deeds: Vec::new(),
            epoch: 0,
            grants: shown.grants.clone(),
            hamlets: shown
                .hamlets
                .iter()
                .map(|place| place.at(now))
                .collect(),
            hand: wire(Hand::NUMBER),
            library: Vec::new(),
            limits: shown.limits.clone().unwrap_or(
                protocol::RealmLimits {
                    budget: protocol::LimitsBudget {
                        cost: None,
                        minutes: None,
                        period: protocol::Period::Day,
                    },
                    roads_between: true,
                },
            ),
            providers: Vec::new(),
            realm: String::new(),
            revision: 0,
            scholars: shown.scholars.clone(),
            settings,
            waits: shown.waits.clone(),
        };
        realm.secrets.scrubbed_value(&state)
    }
}

/// Reads what a client is shown beside `seen`.
pub(super) async fn read(
    rows: &Rows<'_>,
    seen: &Seen,
) -> Result<Shown> {
    let chief = chiefs::named(rows).await?.unwrap_or_default();
    let workers = seen
        .agents
        .iter()
        .filter(|agent| agent.worker.is_some())
        .map(|agent| agent.agent)
        .collect::<BTreeSet<_>>();
    let mut periods = BTreeMap::new();
    for spend in &seen.spend {
        let of = limits::budget(rows, &spend.payer).await?.period;
        periods.insert(
            payer_key(&spend.payer)?,
            Period {
                of,
                began: budget::began(rows, of).await?,
                ends: budget::ends(rows, of).await?,
            },
        );
    }
    let mut agents = Vec::new();
    let mut spent = BTreeMap::new();
    for agent in &seen.agents {
        if matches!(agent.agent.numbered(), Numbered::Errand(_)) {
            continue;
        }
        agents.push(agent_shown(rows, seen, agent, &chief).await?);
        let payer = match &agent.hamlet {
            Some(hamlet) => Payer::Hamlet {
                hamlet: hamlet.clone(),
            },
            None => Payer::Realm,
        };
        if let Some(period) = periods.get(&payer_key(&payer)?) {
            spent.insert(
                agent.agent,
                spent_by(rows, agent.agent, *period).await?,
            );
        }
    }
    let mut hamlets = Vec::new();
    for hamlet in &seen.hamlets {
        let payer = Payer::Hamlet {
            hamlet: hamlet.hamlet.clone(),
        };
        let period = periods.get(&payer_key(&payer)?).copied();
        let spending = seen
            .spend
            .iter()
            .find(|spend| spend.payer == payer)
            .map(|spend| Spending {
                micros: spend.spent_micros,
                ms: spend.spent_ms,
                tokens: 0,
                cap_micros: spend.budget_micros,
                cap_ms: spend.budget_ms,
                period,
            })
            .unwrap_or_default();
        hamlets.push(place(rows, seen, hamlet, spending).await?);
    }
    Ok(Shown {
        agents,
        spent,
        hamlets,
        closed: closed(rows, &chief).await?,
        scholars: scholars(rows, &chief).await?,
        grants: grants(rows, &workers, &chief).await?,
        waits: waits(rows, seen, &workers).await?,
        limits: Some(limits::realm_shown(rows).await?),
        settings: Some(settings_shown(rows).await?),
    })
}

fn payer_key(payer: &Payer) -> Result<String> {
    Ok(serde_json::to_string(payer)?)
}

async fn spent_by(
    rows: &Rows<'_>,
    agent: AgentNumber,
    period: Period,
) -> Result<Spending> {
    let (micros, ms, tokens) = rows
        .row::<(u64, u64, u64)>(
            "SELECT COALESCE(SUM(micros), 0), \
             COALESCE(SUM(took_ms), 0), COALESCE(SUM(tokens), 0) \
             FROM charge WHERE agent = ?1 AND at >= ?2",
            vec![number(agent), BindParam::Integer(period.began)],
        )
        .await?
        .unwrap_or_default();
    Ok(Spending {
        micros,
        ms,
        tokens,
        cap_micros: None,
        cap_ms: None,
        period: Some(period),
    })
}

async fn agent_shown(
    rows: &Rows<'_>,
    seen: &Seen,
    agent: &AgentSeen,
    chief: &str,
) -> Result<protocol::Agent> {
    let n = agent.agent;
    let (kind, since, profile, pin, purpose) =
        rows.row::<(
            String,
            i64,
            Option<u32>,
            Option<String>,
            Option<String>,
        )>(
            "SELECT agent.kind, agent.at, hire.profile, \
             hire.version, COALESCE(hire.purpose, worker.brief) \
             FROM agent LEFT JOIN hire ON hire.agent = agent.id \
             LEFT JOIN worker ON worker.agent = agent.id WHERE \
             agent.id = ?1",
            vec![number(n)],
        )
        .await?
        .unwrap_or_default();
    let worker = kind == "worker";
    let left = match &agent.status {
        Status::Ended { left } => {
            Some(left_shown(rows, n, left, chief).await?)
        }
        _ => None,
    };
    let consulting = rows
        .value::<i64>(
            "SELECT 1 FROM librarian JOIN errand ON errand.n = \
             librarian.errand WHERE librarian.asker = ?1 AND \
             errand.ended IS NULL",
            vec![number(n)],
        )
        .await?
        .is_some();
    let (kind, parent, scholar) = if n == Hand::NUMBER {
        (protocol::AgentKind::Hand, None, Some(HAND_PROFILE.0))
    } else if worker {
        let starter = match agent.worker {
            Some(Starter::Agent { agent }) => Some(agent),
            _ => None,
        };
        (protocol::AgentKind::Worker, starter, None)
    } else {
        (
            protocol::AgentKind::Hire,
            Some(agent.lead.unwrap_or(Hand::NUMBER)),
            profile,
        )
    };
    Ok(protocol::Agent {
        activity: protocol::AgentActivity {
            doing: protocol::Doing::Rest,
            doing_words: String::new(),
            failures: 0,
            step: 0,
        },
        allowance: agent.allowance_micros.map(limits::dollars),
        busy: false,
        consulting: left.is_none() && consulting,
        conversations: peers(rows, n).await?,
        effort: agent.effort,
        faulted: false,
        fast: agent.fast.unwrap_or(false),
        hamlet: agent.hamlet.as_ref().map(|hamlet| hamlet.0.clone()),
        id: wire(n),
        kind,
        mayor: left.is_none()
            && seen.hamlets.iter().any(|place| place.mayor == n),
        left,
        model: agent.runs.clone().unwrap_or_default(),
        name: address(n, worker),
        nickname: agent.nickname.clone(),
        own_model: agent.model.clone(),
        parent: parent.map(wire),
        pin,
        purpose: purpose.unwrap_or_default(),
        saying: protocol::AgentSaying {
            text: String::new(),
            thought: false,
        },
        scholar: scholar
            .map(|profile| protocol::ScholarId(profile.into())),
        since: time(since),
        skills: Vec::new(),
        spent: protocol::AgentSpent {
            cost: money(0),
            micros: 0,
            ms: 0,
            tokens: 0,
        },
        tools_off: agent
            .tools_off
            .iter()
            .map(|tool| tool.name.clone())
            .collect(),
    })
}

/// How `agent` left, as the protocol shows it.
async fn left_shown(
    rows: &Rows<'_>,
    agent: AgentNumber,
    left: &Left,
    chief: &str,
) -> Result<protocol::Left> {
    let by_name = |by: &Option<AgentName>| {
        by.as_ref().map_or_else(
            || chief.to_owned(),
            |by| address(by.agent, false),
        )
    };
    let realm = || "the realm".to_owned();
    let (by, how) = match &left.how {
        HowEnded::ByTheChief => {
            (chief.to_owned(), protocol::LeftHow::Ended)
        }
        HowEnded::ByALead { lead } => {
            (address(lead.agent, false), protocol::LeftHow::Ended)
        }
        HowEnded::ByTheHand => {
            (address(Hand::NUMBER, false), protocol::LeftHow::Ended)
        }
        HowEnded::Answered => (realm(), protocol::LeftHow::Answered),
        HowEnded::Failed => (realm(), protocol::LeftHow::Failed),
        HowEnded::Stopped => (realm(), protocol::LeftHow::Stopped),
        HowEnded::Merged { by, .. } => {
            (by_name(by), protocol::LeftHow::Merged)
        }
        HowEnded::ProfileArchived => {
            (chief.to_owned(), protocol::LeftHow::Scholar)
        }
        HowEnded::HamletRemoved { by } => {
            (by_name(by), protocol::LeftHow::Hamlet)
        }
    };
    let at = rows
        .value::<Option<i64>>(
            "SELECT MAX(at) FROM decision WHERE agent = ?1",
            vec![number(agent)],
        )
        .await?
        .flatten()
        .unwrap_or_default();
    Ok(protocol::Left {
        at: time(at),
        by,
        how,
        into: match &left.how {
            HowEnded::Merged { into, .. } => Some(wire(into.agent)),
            _ => None,
        },
        reason: left.why.clone().unwrap_or_default(),
        words: left.how.to_string(),
    })
}

async fn closed(
    rows: &Rows<'_>,
    chief: &str,
) -> Result<Vec<protocol::ClosedHamlet>> {
    let mut closed = Vec::new();
    for (id, name) in rows
        .rows::<(String, String)>(
            "SELECT id, name FROM hamlet WHERE status = 'removed' \
             ORDER BY name, id",
            Vec::new(),
        )
        .await?
    {
        let (at, by, why) = rows
            .row::<(i64, String, Option<String>)>(
                "SELECT at, by, why FROM decision WHERE hamlet = ?1 \
                 AND kind IN ('removed-hamlet', 'erased-hamlet') \
                 ORDER BY id LIMIT 1",
                vec![BindParam::Text(id.clone())],
            )
            .await?
            .unwrap_or_default();
        closed.push(protocol::ClosedHamlet {
            at,
            by: match by.strip_prefix("agent/").map(str::parse) {
                Some(Ok(agent)) => address(AgentNumber(agent), false),
                _ => chief.to_owned(),
            },
            id,
            name,
            reason: why.unwrap_or_default(),
        });
    }
    Ok(closed)
}

async fn scholars(
    rows: &Rows<'_>,
    chief: &str,
) -> Result<Vec<protocol::Scholar>> {
    let mut scholars = Vec::new();
    for row in rows
        .select(
            "SELECT id, kind, name, status, arrival, house_x, \
             house_z, house_turn FROM profile WHERE kind != 'hand' \
             AND status IN ('living', 'archived') ORDER BY id",
            Vec::new(),
        )
        .await?
    {
        let [id, kind, name, status, arrival, x, z, turn] = &row[..]
        else {
            bail!("a profile's row of another form: {row:?}");
        };
        let id = u32::of(id)?;
        let kind: profiles::Kind =
            change::from_case_name(String::of(kind)?)?;
        let status: profiles::Status =
            change::from_case_name(String::of(status)?)?;
        let arrival: profiles::Arrival =
            change::from_case_name(String::of(arrival)?)?;
        let hires = rows
            .values::<u32>(
                "SELECT hire.agent FROM hire JOIN agent ON agent.id \
                 = hire.agent WHERE hire.profile = ?1 AND \
                 agent.erased = 0 ORDER BY hire.agent",
                vec![BindParam::Integer(id.into())],
            )
            .await?;
        scholars.push(protocol::Scholar {
            arrival: match (kind, arrival) {
                (
                    profiles::Kind::Blank | profiles::Kind::Empty,
                    _,
                ) => protocol::Arrival::Apart,
                (_, profiles::Arrival::Harbor) => {
                    protocol::Arrival::Waiting
                }
                (_, profiles::Arrival::Housed) => {
                    protocol::Arrival::Housed
                }
                (_, profiles::Arrival::Turned) => {
                    protocol::Arrival::Turned
                }
            },
            arrival_by: None,
            chief: chief.to_owned(),
            hires: hires
                .into_iter()
                .map(|hire| wire(AgentNumber(hire)))
                .collect(),
            house: match (
                Option::<f64>::of(x)?,
                Option::<f64>::of(z)?,
            ) {
                (Some(x), Some(z)) => {
                    Some(profiles::house(x, z, Option::of(turn)?))
                }
                _ => None,
            },
            id: protocol::ScholarId(id.into()),
            left: (status == profiles::Status::Archived).then(|| {
                protocol::ScholarLeft {
                    at: 0,
                    by: chief.to_owned(),
                    reason: String::new(),
                }
            }),
            name: String::of(name)?,
            since: 0,
            skills: Vec::new(),
            versions: Vec::new(),
        });
    }
    Ok(scholars)
}

/// The conversations other agents keep with `agent`, by sender and
/// kind: the letters and questions sent it, as today's
/// (`crates/hamlet_server/src/clients/state.rs`, `peers`).
async fn peers(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<protocol::ConversationView>> {
    rows.rows::<(u32, String, i64, Option<i64>)>(
        "SELECT sender, kind, COUNT(*), MAX(at) FROM letter WHERE \
         receiver = ?1 GROUP BY sender, kind ORDER BY MIN(id)",
        vec![number(agent)],
    )
    .await?
    .into_iter()
    .map(|(sender, kind, count, last)| {
        Ok(protocol::ConversationView {
            count,
            kind: match change::from_case_name(kind)? {
                letters::Kind::Letter => {
                    protocol::ConversationViewKind::Letters
                }
                letters::Kind::Question => {
                    protocol::ConversationViewKind::Questions
                }
            },
            last,
            with: wire(AgentNumber(sender)),
        })
    })
    .collect()
}

async fn grants(
    rows: &Rows<'_>,
    workers: &BTreeSet<AgentNumber>,
    chief: &str,
) -> Result<Vec<protocol::Grant>> {
    rows.rows::<(u32, String)>(
        "SELECT agent, permission FROM grant WHERE revoked_at IS \
         NULL ORDER BY id",
        Vec::new(),
    )
    .await?
    .into_iter()
    .map(|(agent, permission)| {
        let agent = AgentNumber(agent);
        Ok(protocol::Grant {
            agent: Some(wire(agent)),
            by: Some(chief.to_owned()),
            name: address(agent, workers.contains(&agent)),
            permission: match change::from_case_name(permission)? {
                Permission::Network => protocol::Permission::Network,
            },
            source: protocol::GrantSource::Grant,
        })
    })
    .collect()
}

async fn waits(
    rows: &Rows<'_>,
    seen: &Seen,
    workers: &BTreeSet<AgentNumber>,
) -> Result<Vec<protocol::Wait>> {
    let mut waits = Vec::new();
    for wait in &seen.waits {
        let (at, conversation) = rows
            .row::<(i64, Option<String>)>(
                "SELECT at, conversation FROM wait WHERE id = ?1",
                vec![change::integer(wait.wait.0)?],
            )
            .await?
            .unwrap_or_default();
        waits.push(wait_shown(
            wait,
            &address(wait.agent, workers.contains(&wait.agent)),
            time(at),
            conversation.is_some(),
        ));
    }
    Ok(waits)
}

/// The words after an agent's address in an approval of `danger`.
fn says(danger: &WaitDanger) -> &'static str {
    match danger {
        WaitDanger::Network => "wants network access to run",
        WaitDanger::Unconfined => "wants to run outside the sandbox:",
        WaitDanger::Reserved => "wants to do what you reserved:",
        WaitDanger::Edit => "wants to edit",
        WaitDanger::Command | WaitDanger::Destructive => {
            "wants to run"
        }
        WaitDanger::Send => "wants to send files:",
    }
}

/// A wait of `by` as the protocol shows it, asked at `at`, with the
/// answers the realm takes for its kind.
fn wait_shown(
    wait: &WaitSeen,
    by: &str,
    at: u64,
    in_chat: bool,
) -> protocol::Wait {
    let approval = |danger: WaitDanger, always: Option<&Lasting>| {
        let always = always.map(|always| match always {
            Lasting::Session => protocol::Lasting::Session,
            Lasting::Grant => protocol::Lasting::Grant,
        });
        let answers = Kind::Tool.answers(always.is_some());
        let says = says(&danger).to_owned();
        (
            answers,
            WaitDetail::Approval {
                always,
                danger,
                says,
            },
        )
    };
    let (answers, detail) = match &wait.kind {
        WaitKind::Tool { danger, always, .. } => approval(
            match danger {
                Need::Network => WaitDanger::Network,
                Need::Edit => WaitDanger::Edit,
                Need::Command => WaitDanger::Command,
            },
            always.as_ref(),
        ),
        WaitKind::Question {
            message, questions, ..
        } => (
            Kind::Question.answers(false),
            WaitDetail::Question {
                questions: protocol::Questions {
                    message: message.clone().unwrap_or_default(),
                    questions: questions
                        .iter()
                        .map(|asked| protocol::Question {
                            description: asked
                                .description
                                .clone()
                                .unwrap_or_default(),
                            options: asked
                                .options
                                .iter()
                                .map(|offer| protocol::Offer {
                                    description: offer
                                        .description
                                        .clone()
                                        .unwrap_or_default(),
                                    title: offer.title.clone(),
                                })
                                .collect(),
                            question: asked.question.clone(),
                            text_input: asked.text_input,
                        })
                        .collect(),
                },
            },
        ),
        WaitKind::Objection { .. } => {
            (Kind::Objection.answers(false), WaitDetail::Objection {})
        }
        WaitKind::Fault => {
            (Kind::Fault.answers(false), WaitDetail::Fault {})
        }
        WaitKind::Reserved => approval(WaitDanger::Reserved, None),
        WaitKind::Send { .. } => approval(WaitDanger::Send, None),
    };
    let mut lines = wait.subject.lines();
    let subject = match &detail {
        WaitDetail::Approval {
            danger: WaitDanger::Edit,
            ..
        } => lines.next().unwrap_or_default(),
        WaitDetail::Objection {} => {
            lines.nth(1).unwrap_or(&wait.subject)
        }
        _ => &wait.subject,
    };
    protocol::Wait {
        about: wait.subject.clone(),
        agent: Some(wire(wait.agent)),
        answers,
        asked: at,
        by: by.to_owned(),
        change: match &wait.kind {
            WaitKind::Tool {
                change: Some(diff), ..
            } => Some(change_shown::shown(diff, &wait.subject)),
            _ => None,
        },
        chat: in_chat.then(|| protocol::ChatRef {
            from: None,
            kind: None,
            past: None,
            with: wire(wait.agent),
        }),
        detail,
        id: wait.wait.0.to_string(),
        since: at,
        subject: subject.to_owned(),
    }
}

async fn settings_shown(
    rows: &Rows<'_>,
) -> Result<protocol::SettingsView> {
    let chosen = rows
        .value::<i64>(
            "SELECT 1 FROM settings WHERE id = 1",
            Vec::new(),
        )
        .await?
        .is_some();
    let kept = settings::read(rows).await?;
    Ok(protocol::SettingsView {
        allow_net: kept.allow_net,
        character: kept.character,
        effort: kept.effort,
        endpoints: kept
            .endpoints
            .into_iter()
            .map(|endpoint| protocol::SettingsViewEndpoints {
                base_url: endpoint.base_url,
                key: false,
                name: endpoint.name,
            })
            .collect(),
        model: kept.model.unwrap_or_default(),
        newcomers: match kept.newcomers {
            Newcomers::Open => protocol::Newcomers::Open,
            Newcomers::Chief => protocol::Newcomers::Chief,
            Newcomers::Hand => protocol::Newcomers::Hand,
        },
        reflect_every: kept.reflect_every.into(),
        unset: !chosen,
        ..unset()
    })
}

/// The settings of a realm no chief has set, before they are read.
fn unset() -> protocol::SettingsView {
    protocol::SettingsView {
        allow_net: false,
        character: String::new(),
        effort: None,
        endpoints: Vec::new(),
        link: protocol::Link {
            models: None,
            state: protocol::LinkState::Untested,
            words: None,
        },
        model: String::new(),
        newcomers: protocol::Newcomers::default(),
        reflect_every: 0,
        unset: true,
        workspace: protocol::SettingsViewWorkspace {
            moving: None,
            running: None,
            url: None,
        },
    }
}

/// `agent`'s address, as in `hire#3`.
fn address(agent: AgentNumber, worker: bool) -> String {
    let kind = if agent == Hand::NUMBER {
        AgentKind::Hand
    } else if worker {
        AgentKind::Worker
    } else {
        AgentKind::Hire
    };
    record::address_of(agent, kind).unwrap_or_default()
}

fn wire(agent: AgentNumber) -> protocol::AgentId {
    protocol::AgentId(agent.0.into())
}

fn time(at: i64) -> u64 {
    u64::try_from(at).unwrap_or_default()
}

fn count(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}
