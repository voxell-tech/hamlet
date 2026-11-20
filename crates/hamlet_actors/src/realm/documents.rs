//! The chief's documents, the charter and the catalogue: the
//! `document` table, their edits, and the copies agents read.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::{Result, ensure};
use hamlet_actor::{BindParam, Ctx};
use hamlet_messages::address::Agent;
use hamlet_messages::agent::copies::{
    Copies, CopyVersion, ModelChoice, Price, Versioned,
};
use hamlet_messages::realm::documents::{
    CardSetting, CatalogueRefusal, CharterRead, EditCatalogue,
    EditCharter, EditRefusal, ImageLimits, ReadCharter, ReadCopies,
};
use hamlet_messages::realm::{Done, Refusable};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision, Unchanged};
use super::calls::Made;
use super::change::{self, Affected, Rows, Writer};
use super::peer::When;
use super::steps::{self, Kind};
use super::tree::choices::{self, Choice, Chosen};
use super::{Realm, chiefs, hire, settings, tree, workers};
use crate::cbor;

const CHARTER: &str =
    include_str!("../../../hamlet_server/assets/charter.md");

/// A part of the copies an agent reads.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Part {
    Charter,
    Model,
    Settings,
}

impl Part {
    const ALL: [Self; 3] =
        [Self::Charter, Self::Model, Self::Settings];
}

/// The agents a moved part of the copies goes to.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub(super) enum Readers {
    Living,
    Agent(AgentNumber),
}

/// A part of the copies a commit moved, with its readers.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub(super) struct Copied {
    pub part: Part,
    pub readers: Readers,
}

/// What the chief says of one model.
#[derive(
    Clone, Debug, Default, PartialEq, Serialize, Deserialize,
)]
#[serde(default)]
pub(super) struct Card {
    /// Absent for what the model's listing says.
    images: Option<ImageLimits>,
    note: String,
    /// Absent for what the model's listing says.
    price: Option<Price>,
}

impl Card {
    fn set(&mut self, setting: &CardSetting) {
        match setting {
            CardSetting::Images { images } => {
                self.images.clone_from(images);
            }
            CardSetting::Note { note } => self.note.clone_from(note),
            CardSetting::Price { price } => {
                self.price.clone_from(price);
            }
        }
    }
}

/// The catalogue's cards, by `provider::model`.
type Catalogue = BTreeMap<String, Card>;

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// The charter a chief wrote; absent gives back the built-in one.
    Charter(Option<String>),
    Card {
        model: String,
        card: Card,
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    let moved = match change {
        Change::Charter(Some(text)) => {
            keep(writer, "charter", text.into_bytes()).await?;
            Some(Part::Charter)
        }
        Change::Charter(None) => {
            writer
                .execute(
                    "DELETE FROM document WHERE name = 'charter'",
                    Vec::new(),
                )
                .await?;
            Some(Part::Charter)
        }
        Change::Card { model, mut card } => {
            card.note = writer.scrubbed(&card.note);
            let mut cards = catalogue(&writer.rows()).await?;
            let price = card.price.clone();
            let before = cards.insert(model.clone(), card);
            keep(writer, "catalogue", cbor(&cards)?).await?;
            let repriced =
                before.and_then(|card| card.price) != price;
            (repriced && runs(&writer.rows(), &model).await?)
                .then_some(Part::Model)
        }
    };
    Ok(Affected {
        copies: moved
            .into_iter()
            .map(|part| Copied {
                part,
                readers: Readers::Living,
            })
            .collect(),
        ..Affected::default()
    })
}

async fn keep(
    writer: &Writer<'_>,
    name: &str,
    value: Vec<u8>,
) -> Result<()> {
    writer
        .execute(
            "INSERT INTO document (name, value) VALUES (?1, ?2) ON \
             CONFLICT (name) DO UPDATE SET value = excluded.value",
            vec![
                BindParam::Text(name.to_owned()),
                BindParam::Blob(value),
            ],
        )
        .await?;
    Ok(())
}

/// Whether `model` is the realm's or any agent's or hamlet's choice.
async fn runs(rows: &Rows<'_>, model: &str) -> Result<bool> {
    if settings::read(rows).await?.model.as_deref() == Some(model) {
        return Ok(true);
    }
    Ok(rows
        .value::<i64>(
            "SELECT 1 FROM model_choice WHERE model = ?1",
            vec![BindParam::Text(model.to_owned())],
        )
        .await?
        .is_some())
}

async fn document(
    rows: &Rows<'_>,
    name: &str,
) -> Result<Option<Vec<u8>>> {
    rows.value(
        "SELECT value FROM document WHERE name = ?1",
        vec![BindParam::Text(name.to_owned())],
    )
    .await
}

async fn own_charter(rows: &Rows<'_>) -> Result<Option<String>> {
    Ok(document(rows, "charter")
        .await?
        .map(String::from_utf8)
        .transpose()?)
}

async fn catalogue(rows: &Rows<'_>) -> Result<Catalogue> {
    Ok(match document(rows, "catalogue").await? {
        Some(bytes) => ciborium::from_reader(bytes.as_slice())?,
        None => Catalogue::new(),
    })
}

/// The agents that are hiring or live and whose actor is made.
pub(super) async fn living(
    rows: &Rows<'_>,
) -> Result<Vec<AgentNumber>> {
    let unmade = [
        steps::not_done(Kind::Hire, hire::Step::Create)?,
        steps::not_done(Kind::Worker, workers::Step::Create)?,
    ];
    Ok(rows
        .values::<u32>(
            &format!(
                "SELECT id FROM agent WHERE status IN ('hiring', \
                 'live') AND id NOT IN ({}) AND id NOT IN ({})",
                unmade[0], unmade[1]
            ),
            Vec::new(),
        )
        .await?
        .into_iter()
        .map(AgentNumber)
        .collect())
}

async fn version(rows: &Rows<'_>, part: Part) -> Result<CopyVersion> {
    Ok(CopyVersion(
        rows.value(
            "SELECT version FROM copy WHERE part = ?1",
            vec![BindParam::Text(change::case_name(&part)?)],
        )
        .await?
        .unwrap_or(0),
    ))
}

/// The model `agent` runs, with its card's price: each part its own
/// choice's, else a worker's starter's, else its hamlet's, else the
/// realm's (`crates/hamlet_server/src/talk/models.rs:96-109`).
pub(super) async fn model(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<ModelChoice>> {
    let chosen = match workers::starter_of(rows, agent).await? {
        Some(starter) => choices::choice(rows, &Chosen::Agent(agent))
            .await?
            .or(choice_of(rows, starter).await?),
        None => choice_of(rows, agent).await?,
    };
    let Some(model) = chosen.model else {
        return Ok(None);
    };
    let price = catalogue(rows)
        .await?
        .remove(&model)
        .and_then(|card| card.price);
    Ok(Some(ModelChoice {
        model,
        effort: chosen.effort,
        fast: chosen.fast.unwrap_or(false),
        price,
    }))
}

/// Each part of `agent`'s own choice, else its hamlet's, else the
/// realm's.
async fn choice_of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Choice> {
    let settings = settings::read(rows).await?;
    let hamlet = match chiefs::hamlet::of_agent(rows, agent).await? {
        Some(hamlet) => {
            choices::choice(rows, &Chosen::Hamlet(hamlet)).await?
        }
        None => Choice::default(),
    };
    let realm = Choice {
        model: settings.model,
        effort: settings.effort,
        fast: None,
    };
    Ok(choices::choice(rows, &Chosen::Agent(agent))
        .await?
        .or(hamlet)
        .or(realm))
}

/// Every part of the copies the realm holds for `agent`, at its
/// version.
pub(super) async fn for_agent(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Copies> {
    newer(rows, agent, &ReadCopies::default()).await
}

/// The parts `reader` reads whose versions differ from those `have`
/// names.
async fn newer(
    rows: &Rows<'_>,
    reader: AgentNumber,
    have: &ReadCopies,
) -> Result<Copies> {
    let mut copies = Copies::default();
    for part in Part::ALL {
        let held = match part {
            Part::Charter => have.charter,
            Part::Model => have.model,
            Part::Settings => have.settings,
        };
        let version = version(rows, part).await?;
        if held != Some(version) {
            fill(rows, &mut copies, part, version, reader).await?;
        }
    }
    Ok(copies)
}

/// Fills `part` of the copies `reader` reads.
async fn fill(
    rows: &Rows<'_>,
    copies: &mut Copies,
    part: Part,
    version: CopyVersion,
    reader: AgentNumber,
) -> Result<()> {
    match part {
        Part::Charter => {
            let text = own_charter(rows).await?;
            copies.charter = Some(Versioned {
                version,
                value: text.unwrap_or_else(|| CHARTER.to_owned()),
            });
        }
        Part::Model => {
            copies.model = model(rows, reader)
                .await?
                .map(|value| Versioned { version, value });
        }
        Part::Settings => {
            copies.settings = Some(Versioned {
                version,
                value: settings::read(rows).await?.for_agents(),
            });
        }
    }
    Ok(())
}

/// Pushes `copied`'s part, as it is now, to its readers.
pub(super) async fn send(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    copied: Copied,
) -> Result<()> {
    let rows = Rows::outside(ctx)?;
    let version = version(&rows, copied.part).await?;
    let readers = match copied.readers {
        Readers::Living => living(&rows).await?,
        Readers::Agent(agent) => {
            if tree::left(&rows, agent).await?.is_some() {
                return Ok(());
            }
            vec![agent]
        }
    };
    let mut failed = BTreeSet::new();
    for reader in readers {
        let mut copies = Copies::default();
        fill(&rows, &mut copies, copied.part, version, reader)
            .await?;
        if copies == Copies::default() {
            continue;
        }
        let sent = realm
            .peers
            .call(&Agent(reader), &copies, When::AfterAnswer)
            .await;
        if let Err(error) = sent {
            tracing::warn!(
                agent = reader.0,
                "copies were not taken: {error:#}"
            );
            failed.insert(reader.0);
        }
    }
    ensure!(failed.is_empty(), "agents {failed:?} took no copies");
    Ok(())
}

impl Act<Client> for EditCharter {
    type Output = Done<EditRefusal>;
    /// The charter a chief wrote, if any.
    type Loaded = Option<String>;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<String>> {
        own_charter(rows).await
    }

    fn decide(
        &self,
        _by: &Client,
        own: Option<String>,
    ) -> Decision<(), Self::Output> {
        let text =
            self.text.clone().filter(|text| !text.trim().is_empty());
        if text == own {
            return Decision::Answer(Done::Done);
        }
        Decision::Change(
            Decided::new(()).change(Change::Charter(text)),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Client> for EditCatalogue {
    type Output = Done<CatalogueRefusal>;
    type Loaded = Catalogue;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Catalogue> {
        catalogue(rows).await
    }

    fn decide(
        &self,
        _by: &Client,
        mut cards: Catalogue,
    ) -> Decision<(), Self::Output> {
        if let Some(refusal) = catalogue_fault(self) {
            return Decision::Answer(Done::refused(refusal));
        }
        let mut card = cards.remove(&self.model).unwrap_or_default();
        for setting in &self.set {
            card.set(setting);
        }
        Decision::Change(Decided::new(()).change(Change::Card {
            model: self.model.clone(),
            card,
        }))
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

fn catalogue_fault(edit: &EditCatalogue) -> Option<CatalogueRefusal> {
    let negative = edit.set.iter().any(|setting| {
        let CardSetting::Price { price: Some(price) } = setting
        else {
            return false;
        };
        [
            price.input,
            price.output,
            price.cache_read,
            price.cache_write,
        ]
        .iter()
        .any(|n| !n.is_finite() || *n < 0.0)
    });
    if negative {
        return Some(CatalogueRefusal::NegativeCost);
    }
    let zero = edit.set.iter().any(|setting| {
        let CardSetting::Images {
            images: Some(images),
        } = setting
        else {
            return false;
        };
        [images.max_width, images.max_height, images.per_line]
            .contains(&Some(0))
            || images.max_bytes == Some(0)
    });
    if zero {
        return Some(CatalogueRefusal::ZeroImageLimit);
    }
    let named = edit.model.split_once("::").is_some_and(
        |(provider, model)| !provider.is_empty() && !model.is_empty(),
    );
    (!named).then(|| CatalogueRefusal::NoSuchModel {
        model: edit.model.clone(),
    })
}

impl Act<Client> for ReadCharter {
    type Output = CharterRead;
    type Loaded = CharterRead;
    type Done = Unchanged;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<CharterRead> {
        let own = own_charter(rows).await?;
        Ok(CharterRead {
            own: own.is_some(),
            text: own.unwrap_or_else(|| CHARTER.to_owned()),
        })
    }

    fn decide(
        &self,
        _by: &Client,
        read: CharterRead,
    ) -> Decision<Unchanged, CharterRead> {
        Decision::Answer(read)
    }

    fn answer(&self, done: Unchanged, _made: &Made) -> CharterRead {
        match done {}
    }
}

impl Act<Agent> for ReadCopies {
    type Output = Copies;
    type Loaded = Copies;
    type Done = Unchanged;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Copies> {
        newer(rows, by.0, self).await
    }

    fn decide(
        &self,
        _by: &Agent,
        copies: Copies,
    ) -> Decision<Unchanged, Copies> {
        Decision::Answer(copies)
    }

    fn answer(&self, done: Unchanged, _made: &Made) -> Copies {
        match done {}
    }
}

#[cfg(test)]
mod tests {
    use hamlet_messages::realm::HamletId;

    use super::super::testing::Started;
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_hire_is_living_once_its_agent_is_made() {
        let started = Started::new().await;
        let agent = AgentNumber(2);
        started.found(&HamletId("h".to_owned()), agent).await;
        let row = hire::row(agent);
        started.commit(steps::Change::Begin { row }).await.unwrap();
        let mut seen = Vec::new();
        let mut from = None;
        for to in ["profile", "branch", "create", "fill"] {
            let rows = started.rows();
            seen.push(living(&rows).await.unwrap().contains(&agent));
            started
                .commit(steps::Change::Advance { row, from, to })
                .await
                .unwrap();
            from = Some(to);
        }

        assert_eq!(seen, [false, false, false, true]);
        started.stop().await;
    }
}
