//! The tree of hires: the `agent`, `hire` and `hire_tool_off`
//! tables, and the lead's powers over them.

pub(super) mod choices;
mod lead;
pub(super) mod tools;

use std::collections::BTreeSet;

use anyhow::{Context as _, Result, bail};
use hamlet_actor::BindParam;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::profile::VersionName;
use hamlet_messages::realm::profiles::ProfileRefusal;
use hamlet_messages::realm::{
    HamletId, HowEnded, Left, Named, ProfileId, ToolName,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{AgentNumber, Numbered};
pub(super) use lead::{Pinned, Pinning, failed};

use super::callers::Decider;
use super::change::{self, Affected, Rows, Writer, integer, number};
use super::world::Part;
use super::{chiefs, profiles, record};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Hiring,
    Live,
    Ended(Left),
}

impl Status {
    pub fn of_row(
        agent: AgentNumber,
        status: &str,
        how: Option<String>,
        why: Option<String>,
    ) -> Result<Self> {
        Ok(match (status, how) {
            ("hiring", _) => Self::Hiring,
            ("live", _) => Self::Live,
            ("ended", Some(how)) => Self::Ended(Left {
                how: serde_json::from_str(&how)?,
                why,
            }),
            (other, how) => {
                bail!("agent {} is `{other}`, ended {how:?}", agent.0)
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Node {
    pub agent: AgentNumber,
    pub kind: AgentKind,
    pub status: Status,
    /// `None` for the Hand and for a mayor.
    pub lead: Option<AgentNumber>,
    pub has_purpose: bool,
}

/// One agent with what a decision about it reads: its leads up to
/// the root of its hamlet, its own hires, and its hamlet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Lineage {
    pub agent: Node,
    /// Its lead first, the mayor last.
    pub leads: Vec<Node>,
    pub hires: Vec<Node>,
    /// `None` for the Hand.
    pub hamlet: Option<chiefs::Hamlet>,
}

impl Node {
    pub fn can_take_purpose(&self) -> bool {
        self.kind == AgentKind::Hire && !self.has_purpose
    }
}

pub(super) fn given_purpose(purpose: Option<&str>) -> Option<String> {
    purpose
        .map(str::trim)
        .filter(|purpose| !purpose.is_empty())
        .map(str::to_owned)
}

const PURPOSE_MOST_CHARS: usize = 200;

pub(super) fn purpose_from(
    hire: AgentNumber,
    text: &str,
) -> Option<Change> {
    let line =
        text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(Change::Purpose {
        hire,
        text: line.chars().take(PURPOSE_MOST_CHARS).collect(),
    })
}

impl Lineage {
    /// Whether `lead` is the agent's lead, or a lead of its leads, or
    /// the Hand (H41). An ended lead leads nothing.
    pub fn is_led_by(&self, lead: AgentNumber) -> bool {
        lead == Hand::NUMBER
            || self.leads.iter().any(|node| {
                node.agent == lead
                    && !matches!(node.status, Status::Ended(_))
            })
    }

    pub fn is_mayor(&self) -> bool {
        self.hamlet
            .as_ref()
            .is_some_and(|hamlet| hamlet.mayor == self.agent.agent)
    }
}

pub(super) async fn lineage(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<Lineage>> {
    let Some(own) = node(rows, agent).await? else {
        return Ok(None);
    };
    let mut leads = Vec::new();
    let mut next = own.lead;
    while let Some(lead) = next {
        let lead = node(rows, lead).await?.with_context(|| {
            format!("the lead {} of a hire", lead.0)
        })?;
        next = lead.lead;
        leads.push(lead);
    }
    let mut hires = Vec::new();
    for hire in rows
        .values::<u32>(
            "SELECT agent FROM hire WHERE lead = ?1",
            vec![number(agent)],
        )
        .await?
    {
        hires.push(
            node(rows, AgentNumber(hire))
                .await?
                .with_context(|| format!("the hire {hire}"))?,
        );
    }
    let hamlet = match chiefs::hamlet::of_agent(rows, agent).await? {
        Some(id) => Some(
            chiefs::hamlet(rows, &id)
                .await?
                .with_context(|| format!("the hamlet `{}`", id.0))?,
        ),
        None => None,
    };
    Ok(Some(Lineage {
        agent: own,
        leads,
        hires,
        hamlet,
    }))
}

fn caller(by: &impl Decider) -> Option<AgentNumber> {
    match by.by() {
        record::By::Agent(agent) => Some(agent),
        record::By::Chief => None,
    }
}

async fn own_hamlet(
    rows: &Rows<'_>,
    caller: Option<AgentNumber>,
) -> Result<Option<HamletId>> {
    match caller {
        Some(caller) => chiefs::hamlet::of_agent(rows, caller).await,
        None => Ok(None),
    }
}

pub(in crate::realm) async fn named(
    rows: &Rows<'_>,
    by: &impl Decider,
    named: &Named,
) -> Result<Option<Lineage>> {
    let own = own_hamlet(rows, caller(by)).await?;
    named_in(rows, own.as_ref(), named).await
}

/// `lineage` with the name a refusal shows its agent by.
pub(in crate::realm) async fn shown(
    rows: &Rows<'_>,
    lineage: Option<Lineage>,
) -> Result<Option<(Lineage, String)>> {
    let Some(lineage) = lineage else {
        return Ok(None);
    };
    let name = record::name_of(rows, lineage.agent.agent).await?;
    Ok(Some((lineage, name)))
}

async fn named_in(
    rows: &Rows<'_>,
    hamlet: Option<&HamletId>,
    named: &Named,
) -> Result<Option<Lineage>> {
    match resolve(rows, hamlet, named).await? {
        Some(agent) => lineage(rows, agent).await,
        None => Ok(None),
    }
}

pub(super) async fn lineage_of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Lineage> {
    lineage(rows, agent)
        .await?
        .with_context(|| format!("agent {} has no row", agent.0))
}

async fn node(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<Node>> {
    let Some((kind, status, how, why, lead, has_purpose)) = rows
        .row::<(
            String,
            String,
            Option<String>,
            Option<String>,
            Option<u32>,
            bool,
        )>(
            "SELECT agent.kind, agent.status, agent.ended_how, \
             agent.ended_why, hire.lead, hire.purpose IS NOT NULL \
             FROM agent LEFT JOIN hire ON hire.agent = agent.id \
             WHERE agent.id = ?1",
            vec![number(agent)],
        )
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(Node {
        agent,
        kind: change::from_case_name(kind)?,
        status: Status::of_row(agent, &status, how, why)?,
        lead: lead.map(AgentNumber),
        has_purpose,
    }))
}

pub(super) async fn resolve(
    rows: &Rows<'_>,
    hamlet: Option<&HamletId>,
    named: &Named,
) -> Result<Option<AgentNumber>> {
    let hamlet = match named {
        Named::Hand => return Ok(Some(Hand::NUMBER)),
        Named::Agent { agent } => {
            return Ok(rows
                .value::<u32>(
                    "SELECT id FROM agent WHERE id = ?1",
                    vec![number(*agent)],
                )
                .await?
                .map(AgentNumber));
        }
        Named::Mayor => hamlet,
        Named::MayorOf { hamlet } => Some(hamlet),
    };
    let Some(hamlet) = hamlet else {
        return Ok(None);
    };
    Ok(chiefs::hamlet(rows, hamlet)
        .await?
        .map(|hamlet| hamlet.mayor))
}

/// The state of a caller's own row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Life {
    /// A hire or the Hand hiring or live, or a worker or an errand
    /// that has not ended.
    Living,
    Ended,
    /// The realm holds no row for the number.
    Unknown,
}

pub(super) async fn life(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Life> {
    let living = match agent.numbered() {
        Numbered::Errand(n) => {
            rows.value::<bool>(
                "SELECT ended IS NULL FROM errand WHERE n = ?1",
                vec![integer(n.get())?],
            )
            .await?
        }
        Numbered::Tree(_) => {
            rows.value::<bool>(
                "SELECT status IN ('hiring', 'live') FROM agent \
                 WHERE id = ?1",
                vec![number(agent)],
            )
            .await?
        }
    };
    Ok(match living {
        Some(true) => Life::Living,
        Some(false) => Life::Ended,
        None => Life::Unknown,
    })
}

pub(super) async fn left(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<Left>> {
    let found = rows
        .row::<(String, Option<String>, Option<String>)>(
            "SELECT status, ended_how, ended_why FROM agent WHERE \
             id = ?1",
            vec![number(agent)],
        )
        .await?;
    let Some((status, how, why)) = found else {
        return Ok(None);
    };
    Ok(match Status::of_row(agent, &status, how, why)? {
        Status::Ended(left) => Some(left),
        Status::Hiring | Status::Live => None,
    })
}

/// Every living hire under `lead`, at any depth: each hire's own
/// hires follow the last of its hires, as today's tree walks them.
pub(super) async fn under(
    rows: &Rows<'_>,
    lead: AgentNumber,
) -> Result<Vec<AgentNumber>> {
    let mut under = Vec::new();
    let mut next = vec![lead];
    while let Some(at) = next.pop() {
        for hire in rows
            .values::<u32>(
                "SELECT hire.agent FROM hire JOIN agent ON agent.id \
                 = hire.agent WHERE hire.lead = ?1 AND agent.status \
                 != 'ended' ORDER BY hire.agent",
                vec![number(at)],
            )
            .await?
        {
            under.push(AgentNumber(hire));
            next.push(AgentNumber(hire));
        }
    }
    Ok(under)
}

/// The hires of `hamlet` that count toward its tier: `hiring` and
/// `live`.
pub(super) async fn living(
    rows: &Rows<'_>,
    hamlet: &HamletId,
) -> Result<u32> {
    Ok(rows
        .value(
            "SELECT COUNT(*) FROM hire JOIN agent ON agent.id = \
             hire.agent WHERE hire.hamlet = ?1 AND agent.status IN \
             ('hiring', 'live')",
            vec![BindParam::Text(hamlet.0.clone())],
        )
        .await?
        .unwrap_or(0))
}

/// The next agent's number, from one sequence: the Hand is 1, the
/// first mayor 2.
pub(super) async fn fresh(rows: &Rows<'_>) -> Result<AgentNumber> {
    Ok(AgentNumber(u32::try_from(rows.next_id("agent").await?)?))
}

/// A hire of the profile `named`, or of a blank one named `name`
/// when none is named.
pub(super) async fn made_of(
    rows: &Rows<'_>,
    named: Option<ProfileId>,
    name: Option<String>,
) -> Result<Result<Of, ProfileRefusal>> {
    let Some(named) = named else {
        return Ok(Ok(blank(rows, name).await?));
    };
    Ok(profiles::shared(rows, named).await?.map(Of::Profile))
}

/// A blank profile named `name`, made for one hire.
pub(super) async fn blank(
    rows: &Rows<'_>,
    name: Option<String>,
) -> Result<Of> {
    Ok(Of::Blank {
        profile: profiles::next(rows).await?,
        name,
    })
}

pub(super) async fn profile_of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<ProfileId> {
    if agent == Hand::NUMBER {
        return Ok(HAND_PROFILE);
    }
    rows.value(
        "SELECT profile FROM hire WHERE agent = ?1",
        vec![number(agent)],
    )
    .await?
    .map(ProfileId)
    .with_context(|| format!("agent {} has no hire row", agent.0))
}

/// The version of its profile a hire stays at; `None` follows
/// `main`.
pub(super) async fn version_of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<VersionName>> {
    Ok(rows
        .value::<Option<String>>(
            "SELECT version FROM hire WHERE agent = ?1",
            vec![number(agent)],
        )
        .await?
        .flatten()
        .map(VersionName))
}

pub(super) async fn kind_of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<AgentKind> {
    let kind = rows
        .value(
            "SELECT kind FROM agent WHERE id = ?1",
            vec![number(agent)],
        )
        .await?
        .with_context(|| format!("agent {} has no row", agent.0))?;
    change::from_case_name(kind)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Of {
    Profile(ProfileId),
    /// A new profile, made for this hire alone (H38), named `name`,
    /// else `Mayor` for a mayor and `Hire <n>` for a hire whose
    /// hamlet had `n` hires before it.
    Blank {
        profile: ProfileId,
        name: Option<String>,
    },
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// Profile 1 and the Hand as agent 1, `hiring`.
    Hand,
    /// A new agent, `hiring`, and its place in a hamlet's tree.
    Hire {
        agent: AgentNumber,
        hamlet: HamletId,
        lead: Option<AgentNumber>,
        of: Of,
        version: Option<VersionName>,
        purpose: Option<String>,
    },
    /// The agent is live.
    Live { agent: AgentNumber },
    /// A hire's purpose, when it has none.
    Purpose { hire: AgentNumber, text: String },
    /// The agent ends, and its hires go to its lead.
    End {
        agent: AgentNumber,
        how: HowEnded,
        why: Option<String>,
    },
    /// `mayor` becomes the root of its hamlet's tree, with the old
    /// mayor under it.
    Seat { mayor: AgentNumber },
    Tools {
        hire: AgentNumber,
        off: Vec<ToolName>,
        on: Vec<ToolName>,
    },
    /// A worker, live from its start and outside every hamlet's tree.
    Worker { agent: AgentNumber },
    /// A worker ends `how`, when it had not.
    WorkerEnded { agent: AgentNumber, how: HowEnded },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Hand => hand(writer).await,
        Change::Hire {
            agent,
            hamlet,
            lead,
            of,
            version,
            purpose,
        } => {
            hire(writer, agent, hamlet, lead, of, version, purpose)
                .await
        }
        Change::Live { agent } => live(writer, agent).await,
        Change::Purpose { hire, text } => {
            purpose(writer, hire, text).await
        }
        Change::End { agent, how, why } => {
            lead::end(writer, agent, how, why).await
        }
        Change::Seat { mayor } => lead::seat(writer, mayor).await,
        Change::Tools { hire, off, on } => {
            tools::set(writer, hire, off, on).await
        }
        Change::Worker { agent } => {
            new_agent(writer, agent, AgentKind::Worker, "live")
                .await?;
            Ok(Affected::seen([Part::Agents]))
        }
        Change::WorkerEnded { agent, how } => {
            worker_ended(writer, agent, how).await
        }
    }
}

pub(super) const HAND_PROFILE: ProfileId = ProfileId(1);

async fn hand(writer: &Writer<'_>) -> Result<Affected> {
    profiles::insert(
        writer,
        HAND_PROFILE,
        profiles::Kind::Hand,
        "hand",
    )
    .await?;
    new_agent(writer, Hand::NUMBER, AgentKind::Hand, "hiring")
        .await?;
    places_showing(writer, Hand::NUMBER, None).await
}

async fn hire(
    writer: &Writer<'_>,
    agent: AgentNumber,
    hamlet: HamletId,
    lead: Option<AgentNumber>,
    of: Of,
    version: Option<VersionName>,
    purpose: Option<String>,
) -> Result<Affected> {
    let profile = match of {
        Of::Profile(profile) => profile,
        Of::Blank { profile, name } => {
            let name = match name.as_deref().map(str::trim) {
                Some(given) if !given.is_empty() => given.to_owned(),
                _ if lead.is_none() => "Mayor".to_owned(),
                _ => format!("Hire {}", had(writer, &hamlet).await?),
            };
            profiles::insert(
                writer,
                profile,
                profiles::Kind::Blank,
                &name,
            )
            .await?;
            profile
        }
    };
    new_agent(writer, agent, AgentKind::Hire, "hiring").await?;
    writer
        .execute(
            "INSERT INTO hire (agent, hamlet, profile, lead, \
             purpose, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            vec![
                number(agent),
                BindParam::Text(hamlet.0),
                BindParam::Integer(i64::from(profile.0)),
                lead.map_or(BindParam::Null, number),
                purpose.map_or(BindParam::Null, BindParam::Text),
                version.map_or(BindParam::Null, |version| {
                    BindParam::Text(version.0)
                }),
            ],
        )
        .await?;
    places_showing(writer, agent, lead).await
}

/// How many hires `hamlet` has had, the ended ones too.
async fn had(writer: &Writer<'_>, hamlet: &HamletId) -> Result<u32> {
    Ok(writer
        .rows()
        .value(
            "SELECT COUNT(*) FROM hire WHERE hamlet = ?1",
            vec![BindParam::Text(hamlet.0.clone())],
        )
        .await?
        .unwrap_or(0))
}

async fn new_agent(
    writer: &Writer<'_>,
    agent: AgentNumber,
    kind: AgentKind,
    status: &str,
) -> Result<()> {
    writer
        .execute(
            "INSERT INTO agent (id, kind, status, at) VALUES (?1, \
             ?2, ?3, ?4)",
            vec![
                number(agent),
                BindParam::Text(change::case_name(&kind)?),
                BindParam::Text(status.to_owned()),
                BindParam::Integer(writer.at().0),
            ],
        )
        .await?;
    Ok(())
}

async fn worker_ended(
    writer: &Writer<'_>,
    agent: AgentNumber,
    how: HowEnded,
) -> Result<Affected> {
    if !set_ended(writer, agent, &how, None).await? {
        return Ok(Affected::default());
    }
    Ok(Affected {
        reaches: BTreeSet::from([agent]),
        ..Affected::seen([Part::Agents, Part::Reaches])
    })
}

/// Ends `agent` as `how`, for `why`; `false` when it had ended.
async fn set_ended(
    writer: &Writer<'_>,
    agent: AgentNumber,
    how: &HowEnded,
    why: Option<String>,
) -> Result<bool> {
    let ended = writer
        .execute(
            "UPDATE agent SET status = 'ended', ended_how = ?2, \
             ended_why = ?3 WHERE id = ?1 AND status != 'ended'",
            vec![
                number(agent),
                BindParam::Text(serde_json::to_string(how)?),
                why.map_or(BindParam::Null, BindParam::Text),
            ],
        )
        .await?;
    Ok(ended > 0)
}

async fn places_showing(
    writer: &Writer<'_>,
    agent: AgentNumber,
    lead: Option<AgentNumber>,
) -> Result<Affected> {
    let places = match lead {
        Some(lead) => BTreeSet::from([lead]),
        None => {
            let mut places = chiefs::mayors(&writer.rows())
                .await?
                .into_iter()
                .collect::<BTreeSet<_>>();
            places.insert(Hand::NUMBER);
            places.remove(&agent);
            places
        }
    };
    Ok(Affected {
        places,
        ..Affected::seen([Part::Agents])
    })
}

async fn live(
    writer: &Writer<'_>,
    agent: AgentNumber,
) -> Result<Affected> {
    let at = vec![number(agent)];
    writer
        .execute(
            "UPDATE agent SET status = 'live' WHERE id = ?1 AND \
             status = 'hiring'",
            at.clone(),
        )
        .await?;
    let lead = writer
        .rows()
        .value::<Option<u32>>(
            "SELECT lead FROM hire WHERE agent = ?1",
            at,
        )
        .await?
        .flatten();
    places_showing(writer, agent, lead.map(AgentNumber)).await
}

async fn purpose(
    writer: &Writer<'_>,
    hire: AgentNumber,
    text: String,
) -> Result<Affected> {
    writer
        .execute(
            "UPDATE hire SET purpose = ?2 WHERE agent = ?1 AND \
             purpose IS NULL",
            vec![number(hire), BindParam::Text(text)],
        )
        .await?;
    Ok(Affected::default())
}

#[cfg(test)]
mod tests {
    use hamlet_actor::BindParam;
    use hamlet_messages::realm::{HowEnded, Left};

    use super::super::testing::Started;
    use super::super::versions;
    use super::*;

    fn agents(nodes: &[Node]) -> Vec<AgentNumber> {
        nodes.iter().map(|node| node.agent).collect()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_lineage_reads_the_leads_up_to_the_mayor() {
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, AgentNumber(2)).await;
        started
            .hire(&hamlet, AgentNumber(3), Some(AgentNumber(2)))
            .await;
        started
            .hire(&hamlet, AgentNumber(4), Some(AgentNumber(3)))
            .await;
        let rows = started.rows();

        let hire =
            lineage(&rows, AgentNumber(4)).await.unwrap().unwrap();
        assert_eq!(
            agents(&hire.leads),
            [AgentNumber(3), AgentNumber(2)]
        );
        assert!(hire.is_led_by(AgentNumber(2)));
        assert!(hire.is_led_by(Hand::NUMBER));
        assert!(!hire.is_led_by(AgentNumber(4)));
        assert!(!hire.is_mayor());
        let mayor =
            lineage(&rows, AgentNumber(2)).await.unwrap().unwrap();
        assert!(mayor.is_mayor());
        assert_eq!(agents(&mayor.hires), [AgentNumber(3)]);
        assert_eq!(hire.hamlet, mayor.hamlet);
        let hand =
            lineage(&rows, Hand::NUMBER).await.unwrap().unwrap();
        assert_eq!(
            (hand.agent.kind, hand.hamlet),
            (AgentKind::Hand, None)
        );
        assert_eq!(
            lineage(&rows, AgentNumber(9)).await.unwrap(),
            None
        );
        started.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_ended_lead_leads_nothing_and_holds_no_place_in_the_tier()
     {
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, AgentNumber(2)).await;
        started
            .hire(&hamlet, AgentNumber(3), Some(AgentNumber(2)))
            .await;
        started
            .hire(&hamlet, AgentNumber(4), Some(AgentNumber(3)))
            .await;
        let left = Left {
            how: HowEnded::ByTheChief,
            why: Some("done".to_owned()),
        };
        started
            .ctx
            .sql()
            .execute(
                "UPDATE agent SET status = 'ended', ended_how = ?1, \
                 ended_why = ?2 WHERE id = 3",
                Some(vec![
                    BindParam::Text(
                        serde_json::to_string(&left.how).unwrap(),
                    ),
                    BindParam::Text("done".to_owned()),
                ]),
            )
            .await
            .unwrap();
        let rows = started.rows();

        assert_eq!(left_of(&rows, 3).await, Some(left.clone()));
        assert_eq!(left_of(&rows, 4).await, None);
        let hire =
            lineage(&rows, AgentNumber(4)).await.unwrap().unwrap();
        assert_eq!(hire.leads[0].status, Status::Ended(left));
        assert!(!hire.is_led_by(AgentNumber(3)));
        assert_eq!(living(&rows, &hamlet).await.unwrap(), 2);
        started.stop().await;
    }

    async fn left_of(rows: &Rows<'_>, agent: u32) -> Option<Left> {
        left(rows, AgentNumber(agent)).await.unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_name_resolves_to_its_agent() {
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, AgentNumber(2)).await;
        started
            .hire(&hamlet, AgentNumber(3), Some(AgentNumber(2)))
            .await;
        let rows = started.rows();
        let named = async |hamlet: Option<&HamletId>,
                           named: Named| {
            resolve(&rows, hamlet, &named).await.unwrap()
        };

        assert_eq!(
            named(None, Named::Hand).await,
            Some(Hand::NUMBER)
        );
        assert_eq!(
            named(Some(&hamlet), Named::Mayor).await,
            Some(AgentNumber(2))
        );
        assert_eq!(named(None, Named::Mayor).await, None);
        let mayor_of = |hamlet: &str| Named::MayorOf {
            hamlet: HamletId(hamlet.to_owned()),
        };
        assert_eq!(
            named(None, mayor_of("h")).await,
            Some(AgentNumber(2))
        );
        assert_eq!(named(None, mayor_of("x")).await, None);
        let agent = |n| Named::Agent {
            agent: AgentNumber(n),
        };
        assert_eq!(named(None, agent(3)).await, Some(AgentNumber(3)));
        assert_eq!(named(None, agent(9)).await, None);
        started.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_hire_is_made_of_a_shared_profile_named_else_a_blank_one()
     {
        let started = Started::new().await;
        let rows = started.rows();
        let of =
            async |named| made_of(&rows, named, None).await.unwrap();
        let no_such = |profile| ProfileRefusal::NoSuchProfile {
            profile: ProfileId(profile),
        };

        assert_eq!(
            of(None).await,
            Ok(Of::Blank {
                profile: ProfileId(2),
                name: None,
            })
        );
        assert_eq!(of(Some(HAND_PROFILE)).await, Err(no_such(1)));
        assert_eq!(of(Some(ProfileId(9))).await, Err(no_such(9)));
        started.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_worker_is_live_at_once_and_ends_once() {
        let started = Started::new().await;
        let worker = fresh(&started.rows()).await.unwrap();
        started
            .commit(Change::Worker { agent: worker })
            .await
            .unwrap();
        let rows = started.rows();
        let node = lineage(&rows, worker).await.unwrap().unwrap();
        assert_eq!(
            (node.agent.kind, node.agent.status, node.hamlet),
            (AgentKind::Worker, Status::Live, None)
        );
        assert_eq!(
            fresh(&rows).await.unwrap(),
            AgentNumber(worker.0 + 1)
        );

        for how in [HowEnded::Answered, HowEnded::Stopped] {
            started
                .commit(Change::WorkerEnded { agent: worker, how })
                .await
                .unwrap();
        }
        let left = left_of(&started.rows(), worker.0).await;
        assert_eq!(
            left,
            Some(Left {
                how: HowEnded::Answered,
                why: None,
            })
        );
        started.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_hire_goes_live_and_keeps_its_first_purpose() {
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, AgentNumber(2)).await;
        started
            .hire(&hamlet, AgentNumber(3), Some(AgentNumber(2)))
            .await;
        let hire = AgentNumber(3);
        let lead_place =
            versions::place(&started.rows(), AgentNumber(2))
                .await
                .unwrap();
        started.commit(Change::Live { agent: hire }).await.unwrap();
        for text in ["first", "second"] {
            started
                .commit(Change::Purpose {
                    hire,
                    text: text.to_owned(),
                })
                .await
                .unwrap();
        }
        let rows = started.rows();

        let node = lineage(&rows, hire).await.unwrap().unwrap().agent;
        assert_eq!(
            (node.status, node.has_purpose),
            (Status::Live, true)
        );
        let purpose = rows
            .value::<String>(
                "SELECT purpose FROM hire WHERE agent = 3",
                Vec::new(),
            )
            .await
            .unwrap();
        assert_eq!(purpose.as_deref(), Some("first"));
        assert_eq!(
            versions::place(&rows, AgentNumber(2)).await.unwrap(),
            lead_place + 1
        );
        assert_eq!(
            profile_of(&rows, hire).await.unwrap(),
            HAND_PROFILE
        );
        assert_eq!(
            profile_of(&rows, Hand::NUMBER).await.unwrap(),
            HAND_PROFILE
        );
        assert_eq!(
            kind_of(&rows, hire).await.unwrap(),
            AgentKind::Hire
        );
        assert_eq!(
            kind_of(&rows, Hand::NUMBER).await.unwrap(),
            AgentKind::Hand
        );
        started.stop().await;
    }
}
