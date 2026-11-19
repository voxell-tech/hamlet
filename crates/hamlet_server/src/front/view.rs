//! The realm a client is shown.

use std::sync::Arc;

use anyhow::Result;
use hamlet_messages::address::Realm;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::client::ReadWorld;
use hamlet_messages::realm::oversight::Permission;
use hamlet_messages::realm::world::{
    AgentSeen, GrantSeen, HamletSeen, Seen, Spend, Starter, Status,
    WaitKind, WaitSeen,
};
use hamlet_messages::realm::{HowEnded, Left, Tier};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    self as protocol, Choice, Confinement, Doing, Link, LinkState,
    Newcomers, SettingsView, SettingsViewWorkspace, WaitDanger,
    WaitDetail,
};
use hamlet_store::ids::AgentId;

use super::call::Calls;
use super::host::HAND;
use super::words;
use crate::clients::state::{hamlet_limits, realm_limits};
use crate::oversight::Danger;
use crate::society::addresses::Address;
use crate::society::agents::EndHow;
use crate::society::limits::{Limits, RealmLimits};
use crate::spend;
use crate::talk::activity;

/// What the server knows of itself, shown in every `State`.
pub(crate) struct Own {
    pub(crate) chief: String,
    pub(crate) confinement: Confinement,
    /// The realm's folder, as a client shows it.
    pub(crate) realm: String,
    /// When this server started, in milliseconds since the epoch.
    pub(crate) epoch: i64,
}

/// The realm as the door last read it, with the server's own fields.
pub(crate) struct View {
    pub(crate) own: Arc<Own>,
    pub(crate) world: Seen,
}

impl View {
    /// The agent numbered `number`, ended ones too.
    pub(crate) fn agent(
        &self,
        number: AgentNumber,
    ) -> Option<&AgentSeen> {
        self.world.agents.iter().find(|seen| seen.agent == number)
    }

    /// The address of the agent numbered `number`: `hand`,
    /// `hire#<n>` or `worker#<n>`.
    pub(crate) fn address(&self, number: AgentNumber) -> String {
        let id = AgentId(i64::from(number.0));
        match self.kind(number) {
            protocol::AgentKind::Hand => Address::Hand,
            protocol::AgentKind::Hire => Address::Hire(id),
            protocol::AgentKind::Worker => Address::Worker(id),
        }
        .to_string()
    }

    /// What `payer` spent in its period, as the world holds it.
    pub(crate) fn spend(&self, payer: &Payer) -> Option<&Spend> {
        self.world.spend.iter().find(|spend| spend.payer == *payer)
    }

    fn kind(&self, number: AgentNumber) -> protocol::AgentKind {
        if number == HAND {
            protocol::AgentKind::Hand
        } else if self
            .agent(number)
            .is_some_and(|seen| seen.worker.is_some())
        {
            protocol::AgentKind::Worker
        } else {
            protocol::AgentKind::Hire
        }
    }
}

/// Reads the realm's world now.
pub(crate) async fn read(
    calls: &Calls,
    own: &Arc<Own>,
) -> Result<View> {
    Ok(View {
        own: own.clone(),
        world: calls.call(&Realm, &ReadWorld).await?,
    })
}

/// The realm as a client is shown it: the server's own fields, and
/// of the agents, hamlets, waits and grants what the world holds.
pub(crate) fn state(view: &View) -> protocol::State {
    let own = &view.own;
    let world = &view.world;
    protocol::State {
        agents: world
            .agents
            .iter()
            .map(|seen| agent(view, seen))
            .collect(),
        chief: own.chief.clone(),
        closed: Vec::new(),
        confinement: own.confinement.clone(),
        deeds: crate::commands::deeds(),
        epoch: own.epoch,
        grants: world
            .grants
            .iter()
            .map(|seen| grant(view, seen))
            .collect(),
        hamlets: world
            .hamlets
            .iter()
            .map(|seen| hamlet(view, seen))
            .collect(),
        hand: wire(HAND),
        library: Vec::new(),
        limits: realm_limits(&RealmLimits::default()),
        providers: Vec::new(),
        realm: own.realm.clone(),
        revision: i64::try_from(world.revision.0).unwrap_or(i64::MAX),
        scholars: Vec::new(),
        settings: SettingsView {
            allow_net: false,
            character: String::new(),
            effort: None,
            endpoints: Vec::new(),
            link: Link {
                models: None,
                state: LinkState::Untested,
                words: None,
            },
            model: String::new(),
            newcomers: Newcomers::default(),
            reflect_every: 0,
            unset: true,
            workspace: SettingsViewWorkspace {
                moving: None,
                running: None,
                url: None,
            },
        },
        waits: world
            .waits
            .iter()
            .map(|seen| wait(view, seen))
            .collect(),
    }
}

/// The agent numbered `number` as the protocol names it.
pub(crate) fn wire(number: AgentNumber) -> protocol::AgentId {
    protocol::AgentId(u64::from(number.0))
}

fn agent(view: &View, seen: &AgentSeen) -> protocol::Agent {
    let number = seen.agent;
    let kind = view.kind(number);
    let left = match &seen.status {
        Status::Ended { left } => Some(gone(view, left)),
        _ => None,
    };
    let mayor = left.is_none()
        && view
            .world
            .hamlets
            .iter()
            .any(|place| place.mayor == number);
    let parent = match kind {
        protocol::AgentKind::Hand => None,
        protocol::AgentKind::Hire => Some(seen.lead.unwrap_or(HAND)),
        protocol::AgentKind::Worker => match &seen.worker {
            Some(Starter::Agent { agent }) => Some(*agent),
            _ => None,
        },
    };
    let doing = Doing::Rest;
    protocol::Agent {
        activity: protocol::AgentActivity {
            doing_words: activity::words(&doing).into(),
            doing,
            failures: 0,
            step: 0,
        },
        allowance: seen
            .allowance_micros
            .map(|micros| micros as f64 / 1_000_000.0),
        busy: false,
        consulting: false,
        conversations: Vec::new(),
        effort: seen.effort,
        faulted: false,
        fast: seen.fast.unwrap_or(false),
        hamlet: seen.hamlet.as_ref().map(|hamlet| hamlet.0.clone()),
        id: wire(number),
        kind,
        left,
        mayor,
        model: seen.model.clone().unwrap_or_default(),
        name: view.address(number),
        nickname: seen.nickname.clone(),
        own_model: seen.model.clone(),
        parent: parent.map(wire),
        pin: None,
        purpose: String::new(),
        saying: protocol::AgentSaying {
            text: String::new(),
            thought: false,
        },
        scholar: None,
        since: 0,
        skills: Vec::new(),
        spent: protocol::AgentSpent {
            cost: spend::money(0),
            micros: 0,
            ms: 0,
            tokens: 0,
        },
        tools_off: Vec::new(),
    }
}

/// How an agent left, as the protocol shows it.
fn gone(view: &View, left: &Left) -> protocol::Left {
    let realm = || words::REALM.to_owned();
    let (end, by, how) = match &left.how {
        HowEnded::ByTheChief => (
            EndHow::Ended,
            view.own.chief.clone(),
            protocol::LeftHow::Ended,
        ),
        HowEnded::ByALead { lead } => (
            EndHow::Ended,
            view.address(*lead),
            protocol::LeftHow::Ended,
        ),
        HowEnded::ByTheHand => (
            EndHow::Ended,
            view.address(HAND),
            protocol::LeftHow::Ended,
        ),
        HowEnded::Answered => {
            (EndHow::Answered, realm(), protocol::LeftHow::Answered)
        }
        HowEnded::Failed => {
            (EndHow::Failed, realm(), protocol::LeftHow::Failed)
        }
        HowEnded::Stopped => {
            (EndHow::Stopped, realm(), protocol::LeftHow::Stopped)
        }
    };
    protocol::Left {
        at: 0,
        by,
        how,
        into: None,
        reason: left.why.clone().unwrap_or_default(),
        words: end.words().into(),
    }
}

fn hamlet(view: &View, seen: &HamletSeen) -> protocol::Hamlet {
    let payer = Payer::Hamlet {
        hamlet: seen.hamlet.clone(),
    };
    protocol::Hamlet {
        at: None,
        before: Vec::new(),
        budget: report(view.spend(&payer), "Hamlet"),
        chiefs: Vec::new(),
        folder: None,
        id: seen.hamlet.0.clone(),
        limits: hamlet_limits(&Limits::default()),
        mayor: wire(seen.mayor),
        mode: protocol::HamletMode::Manual,
        mode_words: words::NO_FOLDER.into(),
        name: seen.name.clone(),
        roads: Vec::new(),
        tier: match seen.tier {
            Tier::Fifty => 50,
            Tier::Hundred => 100,
            Tier::TwoHundred => 200,
            Tier::FiveHundred => 500,
        },
    }
}

/// The budget report of `payer`, from its spend in the world.
fn report(spend: Option<&Spend>, whose: &str) -> protocol::Report {
    let count = |value: u64| i64::try_from(value).unwrap_or(i64::MAX);
    let spent = protocol::Amount {
        micros: spend.map_or(0, |spend| count(spend.spent_micros)),
        ms: spend.map_or(0, |spend| count(spend.spent_ms)),
        tokens: 0,
    };
    let caps = protocol::ReportCaps {
        micros: spend
            .and_then(|spend| spend.budget_micros)
            .map(count),
        ms: spend.and_then(|spend| spend.budget_ms).map(count),
    };
    protocol::Report {
        summary: words::budget(whose, &spent, &caps),
        caps,
        gone: 0.0,
        left: None,
        lines: Vec::new(),
        period: protocol::Period::Day,
        period_words: words::TODAY.into(),
        remains: None,
        spent,
    }
}

fn grant(view: &View, seen: &GrantSeen) -> protocol::Grant {
    protocol::Grant {
        agent: Some(wire(seen.agent)),
        by: None,
        name: view.address(seen.agent),
        permission: match seen.permission {
            Permission::Network => protocol::Permission::Network,
        },
        source: protocol::GrantSource::Grant,
    }
}

/// A wait as the protocol shows it, with the answers the realm
/// takes for its kind.
fn wait(view: &View, seen: &WaitSeen) -> protocol::Wait {
    let approval = |danger: Danger, wire: WaitDanger| {
        (
            vec![Choice::Allow, Choice::Deny],
            WaitDetail::Approval {
                always: None,
                danger: wire,
                says: danger.says().into(),
            },
        )
    };
    let (answers, detail) = match &seen.kind {
        WaitKind::Question {
            message, questions, ..
        } => (
            vec![Choice::Reply, Choice::Dismiss],
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
        WaitKind::Objection { .. } => (
            vec![Choice::Go, Choice::Withdraw],
            WaitDetail::Objection {},
        ),
        WaitKind::Fault => {
            (vec![Choice::Dismiss], WaitDetail::Fault {})
        }
        WaitKind::Reserved => {
            approval(Danger::Reserved, WaitDanger::Reserved)
        }
        WaitKind::Send { .. } => {
            approval(Danger::Send, WaitDanger::Send)
        }
        _ => approval(Danger::Command, WaitDanger::Command),
    };
    protocol::Wait {
        about: seen.subject.clone(),
        agent: Some(wire(seen.agent)),
        answers,
        asked: 0,
        by: view.address(seen.agent),
        change: None,
        chat: None,
        detail,
        id: seen.wait.0.to_string(),
        since: 0,
        subject: seen.subject.clone(),
    }
}
