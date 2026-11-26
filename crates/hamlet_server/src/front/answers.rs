//! The answers to the reads and edits a client asks of the profiles,
//! the library and the agents.

use anyhow::Result;
use base64::Engine as _;
use hamlet_messages::address::{self, Profile};
use hamlet_messages::agent::chat::{EditMemory, ReadPapers};
use hamlet_messages::agent::copies::Price;
use hamlet_messages::bytes::Sha256;
use hamlet_messages::profile::{self, ReadAnswer};
use hamlet_messages::realm::budget::{self, BudgetLimit};
use hamlet_messages::realm::choices::{ModelOf, SetModel};
use hamlet_messages::realm::client::ReadState;
use hamlet_messages::realm::documents::{
    CardSetting, EditCatalogue, EditCharter, EditSettings, Endpoint,
    ImageLimits, Newcomers, RemoteWorkspace, Setting, SettingRefusal,
};
use hamlet_messages::realm::profiles::EditProfile;
use hamlet_messages::realm::{
    Done, HamletId, Limit, ProfileId, ReservedAct, Review, SetLimits,
};
use hamlet_protocol::client::{
    self as protocol, EditLimits, ImageBytes, ImageId, ImageSize,
    Keeper, ReadResult, RequestId, ScholarId, ServerMessage,
};
use serde_json::Value;

use super::call::Calls;
use super::commands::{Outcome, ask, derived, done, refused};
use super::uploads::Uploads;
use super::{route, words};
use crate::clients::check::reply;
use crate::clients::documents::{budget_of, limits_of, memory_path};
use crate::images::{
    Bounds, Format, THUMBNAIL_BYTES, THUMBNAIL_SIDE, capitalised, fit,
};
use crate::knowledge::CHARACTER;
use crate::society::city::CHARACTER_MOST;
use crate::society::refusal::Refusal;
use crate::spend;

const THUMBNAIL: Bounds = Bounds {
    width: THUMBNAIL_SIDE,
    height: THUMBNAIL_SIDE,
    bytes: THUMBNAIL_BYTES,
};

/// A read the door answers from more than one call.
pub(crate) enum Reading {
    Papers {
        id: RequestId,
        of: Keeper,
    },
    Image {
        id: RequestId,
        image: ImageId,
        size: ImageSize,
    },
}

impl Reading {
    pub(crate) fn id(&self) -> &RequestId {
        match self {
            Self::Papers { id, .. } | Self::Image { id, .. } => id,
        }
    }
}

pub(crate) async fn read(
    calls: &Calls,
    uploads: &Uploads,
    reading: Reading,
) -> Result<ServerMessage> {
    match reading {
        Reading::Papers { id, of } => {
            Ok(match papers(calls, of).await? {
                Ok(papers) => {
                    ServerMessage::Read(ReadResult::Papers {
                        papers,
                        to: id,
                    })
                }
                Err(refusal) => reply(&id, Err(refusal)),
            })
        }
        Reading::Image { id, image, size } => {
            image_read(uploads, id, image, size).await
        }
    }
}

/// The profile `scholar` names and its label, if the realm keeps one
/// by that number.
pub(crate) async fn profile(
    calls: &Calls,
    scholar: ScholarId,
) -> Result<Result<(ProfileId, String), Refusal>> {
    let state = calls.call(&address::Realm, &ReadState).await?;
    let missing = || Refusal::NoScholar {
        address: format!("scholar#{}", scholar.0),
    };
    Ok(state
        .scholars
        .iter()
        .find(|shown| shown.id == scholar)
        .and_then(|shown| {
            let profile = u32::try_from(shown.id.0).ok()?;
            let label = format!("scholar#{profile} ({})", shown.name);
            Some((ProfileId(profile), label))
        })
        .ok_or_else(missing))
}

/// What `of` knows: a profile's `main`, or an agent's own files.
async fn papers(
    calls: &Calls,
    of: Keeper,
) -> Result<Result<protocol::Papers, Refusal>> {
    let scholar = match of {
        Keeper::Agent(agent) => {
            let Some(number) = route::number(agent) else {
                return Ok(Err(Refusal::NoOne {
                    address: agent.0.to_string(),
                }));
            };
            return Ok(Ok(calls
                .call(&address::Agent(number), &ReadPapers)
                .await?));
        }
        Keeper::Scholar(scholar) => scholar,
    };
    let profile = match profile(calls, scholar).await? {
        Ok((profile, _)) => Profile(profile),
        Err(refusal) => return Ok(Err(refusal)),
    };
    let list = calls.call(&profile, &profile::Papers).await?;
    let mut memory = Vec::new();
    for paper in list.memory {
        let read = profile::Read {
            at: None,
            path: Some(paper.path.clone()),
        };
        let text = match calls.call(&profile, &read).await? {
            ReadAnswer::Text { text, .. } => text,
            _ => String::new(),
        };
        memory.push(protocol::MemoryFile {
            description: paper.description,
            path: under(&paper.path, "memory/").to_owned(),
            text,
        });
    }
    let skills = list
        .skills
        .into_iter()
        .map(|paper| protocol::Skill {
            name: under(&paper.path, "skills/")
                .split('/')
                .next()
                .unwrap_or_default()
                .to_owned(),
            description: paper.description,
        })
        .collect();
    Ok(Ok(protocol::Papers {
        character: list.character,
        learned: Vec::new(),
        memory,
        skills,
    }))
}

fn under<'a>(path: &'a str, folder: &str) -> &'a str {
    path.strip_prefix(folder).unwrap_or(path)
}

async fn image_read(
    uploads: &Uploads,
    id: RequestId,
    image: ImageId,
    size: ImageSize,
) -> Result<ServerMessage> {
    let missing = || {
        reply(
            &id,
            Err(Refusal::NoImage {
                image: image.to_string(),
            }),
        )
    };
    let Some(sha256) = Sha256::from_hex(image.as_str()) else {
        return Ok(missing());
    };
    let path = uploads.store().file(&sha256);
    let found = tokio::task::spawn_blocking(move || {
        let Ok(bytes) = std::fs::read(path) else {
            return None;
        };
        let format = Format::detect(&bytes)?;
        Some(match size {
            ImageSize::Original => (bytes, format),
            ImageSize::Thumbnail => {
                match fit(&bytes, format, THUMBNAIL) {
                    Ok(made) => (made.bytes, made.format),
                    Err(_) => (bytes, format),
                }
            }
        })
    })
    .await?;
    let Some((bytes, format)) = found else {
        return Ok(missing());
    };
    Ok(ServerMessage::Read(ReadResult::Image {
        image: ImageBytes {
            data: base64::engine::general_purpose::STANDARD
                .encode(&bytes),
            mime: format.mime().into(),
        },
        to: id,
    }))
}

pub(crate) async fn edit(
    calls: &Calls,
    edit: &protocol::Edit,
) -> Result<Outcome> {
    let reply = match edit {
        protocol::Edit::Charter { id, text } => {
            let text =
                text.clone().filter(|text| !text.trim().is_empty());
            let answer = ask(calls, id, EditCharter { text }).await?;
            done(answer, || words::CHARTER_SAVED.into())
        }
        protocol::Edit::Catalogue { id, model, set } => {
            let act = EditCatalogue {
                model: model.clone(),
                set: card(set),
            };
            done(ask(calls, id, act).await?, || {
                words::CATALOGUE_SAVED.into()
            })
        }
        protocol::Edit::Character { id, scholar, text } => {
            if text.chars().count() > CHARACTER_MOST {
                Err(Refusal::LongCharacter)
            } else {
                profile_edit(
                    calls,
                    id,
                    *scholar,
                    CHARACTER,
                    text,
                    words::CHARACTER_SAVED,
                )
                .await?
            }
        }
        protocol::Edit::Memory { id, of, path, text } => {
            match (memory_path(path), of) {
                (Err(refusal), _) => Err(refusal),
                (Ok(path), Keeper::Scholar(scholar)) => {
                    profile_edit(
                        calls,
                        id,
                        *scholar,
                        &path,
                        text,
                        words::MEMORY_SAVED,
                    )
                    .await?
                }
                (Ok(path), Keeper::Agent(agent)) => {
                    match route::number(*agent) {
                        Some(number) => {
                            let edit = EditMemory {
                                path,
                                text: text.clone(),
                            };
                            calls
                                .call(&address::Agent(number), &edit)
                                .await?;
                            Ok(words::MEMORY_SAVED.into())
                        }
                        None => Err(Refusal::NoOne {
                            address: agent.0.to_string(),
                        }),
                    }
                }
            }
        }
        protocol::Edit::Limits(edit) => limits(calls, edit).await?,
        protocol::Edit::Settings { id, set } => {
            settings(calls, id, set).await?
        }
    };
    Ok(Outcome::said(reply))
}

/// Writes `text` to the file `path` of the profile `scholar` names,
/// in the words `saved` once written.
async fn profile_edit(
    calls: &Calls,
    id: &RequestId,
    scholar: ScholarId,
    path: &str,
    text: &str,
    saved: &str,
) -> Result<Result<String, Refusal>> {
    let Ok(profile) = u32::try_from(scholar.0).map(ProfileId) else {
        return Ok(Err(no_scholar(scholar)));
    };
    let act = EditProfile {
        profile,
        path: path.to_owned(),
        text: text.to_owned(),
        message: None,
    };
    Ok(done(ask(calls, id, act).await?, || saved.to_owned()))
}

fn no_scholar(scholar: ScholarId) -> Refusal {
    Refusal::NoScholar {
        address: format!("scholar#{}", scholar.0),
    }
}

/// Sets the limits of a hamlet, or the realm's, to what `edit` makes
/// of their client form as the realm shows them.
async fn limits(
    calls: &Calls,
    edit: &EditLimits,
) -> Result<Result<String, Refusal>> {
    let (hamlet, id) = match edit {
        EditLimits::Set { hamlet, id, .. }
        | EditLimits::Whole { hamlet, id, .. } => (hamlet, id),
    };
    let edited = |mut current: Value| match edit {
        EditLimits::Whole { whole, .. } => {
            Value::Object(whole.clone().into_iter().collect())
        }
        EditLimits::Set { set, .. } => {
            match current
                .get_mut(&set.section)
                .and_then(Value::as_object_mut)
            {
                Some(section) => {
                    section
                        .insert(set.key.clone(), set.value.clone());
                }
                None => current[&set.key] = set.value.clone(),
            }
            current
        }
    };
    let unfit = || {
        Ok(Err(Refusal::Usage {
            words: words::LIMITS_UNFIT.into(),
        }))
    };
    let state = calls.call(&address::Realm, &ReadState).await?;
    let Some(hamlet) = hamlet else {
        let Ok(limits) =
            serde_json::from_value::<protocol::RealmLimits>(edited(
                serde_json::to_value(&state.limits)?,
            ))
        else {
            return unfit();
        };
        if let Some(words) = budget_of(&limits.budget).fault() {
            return Ok(Err(Refusal::Usage { words }));
        }
        let act = SetLimits {
            hamlet: None,
            budget: budget(&limits.budget),
            limits: vec![Limit::RoadsBetween {
                allowed: limits.roads_between,
            }],
        };
        return Ok(done(ask(calls, id, act).await?, || {
            words::REALM_LIMITS_SAVED.into()
        }));
    };
    let Some(shown) =
        state.hamlets.iter().find(|shown| shown.id == *hamlet)
    else {
        return Ok(Err(Refusal::NoHamlet { id: hamlet.clone() }));
    };
    let Ok(limits) = serde_json::from_value::<protocol::Limits>(
        edited(serde_json::to_value(&shown.limits)?),
    ) else {
        return unfit();
    };
    if let Some(words) = limits_of(&limits).fault() {
        return Ok(Err(Refusal::Usage { words }));
    }
    let Some(set) = hamlet_limits(&limits) else {
        return unfit();
    };
    let hamlet_id = HamletId(hamlet.clone());
    let act = SetLimits {
        hamlet: Some(hamlet_id.clone()),
        budget: budget(&limits.budget),
        limits: set,
    };
    let saved = words::limits_saved(hamlet);
    if let Done::Refused { why } = ask(calls, id, act).await? {
        return Ok(Err(refused(why)));
    }
    if limits.model == shown.limits.model {
        return Ok(Ok(saved));
    }
    let model = SetModel {
        of: ModelOf::Hamlet { hamlet: hamlet_id },
        model: limits.model,
        effort: None,
        fast: None,
        why: None,
    };
    let id = derived(id, *b"md");
    Ok(done(ask(calls, &id, model).await?, || saved))
}

fn budget(shown: &protocol::LimitsBudget) -> Vec<BudgetLimit> {
    let cost = shown.cost.map_or(BudgetLimit::NoCost, |dollars| {
        BudgetLimit::Cost {
            micros: u64::try_from(spend::micros(dollars))
                .unwrap_or(0),
        }
    });
    let time = shown.minutes.map_or(BudgetLimit::NoTime, |minutes| {
        BudgetLimit::Time {
            ms: u64::try_from(minutes).unwrap_or(0) * 60_000,
        }
    });
    let period = BudgetLimit::Period {
        period: match shown.period {
            protocol::Period::Day => budget::Period::Day,
            protocol::Period::Month => budget::Period::Month,
        },
    };
    vec![cost, time, period]
}

/// A hamlet's limits beside its budget and model as the realm sets
/// them, none when a number does not fit.
fn hamlet_limits(shown: &protocol::Limits) -> Option<Vec<Limit>> {
    let count = |value: i64| u32::try_from(value).ok();
    let reserved = |act, reserved| Limit::Reserved { act, reserved };
    Some(vec![
        Limit::Questions {
            most: count(shown.asking.questions)?,
        },
        Limit::Refusals {
            most: count(shown.asking.refusals)?,
        },
        Limit::Review {
            review: match shown.asking.review {
                protocol::LimitsReview::Always => Review::Always,
                protocol::LimitsReview::Remote => Review::Remote,
                protocol::LimitsReview::Never => Review::Never,
            },
        },
        Limit::ProtectedMinutes {
            minutes: count(shown.protected_minutes)?,
        },
        Limit::ToldDays {
            days: count(shown.told_days)?,
        },
        reserved(ReservedAct::End, shown.reserved.end),
        reserved(ReservedAct::Hire, shown.reserved.hire),
        reserved(ReservedAct::Merge, shown.reserved.merge),
        reserved(ReservedAct::Roads, shown.reserved.roads),
        Limit::RoadPrice {
            micros: u64::try_from(spend::micros(shown.road.price))
                .ok()?,
        },
        Limit::RoadsMost {
            most: count(shown.road.most)?,
        },
        Limit::RoadsGrowOverDays {
            days: count(shown.road.grows_over_days)?,
        },
    ])
}

async fn settings(
    calls: &Calls,
    id: &RequestId,
    set: &protocol::EditSettingsSet,
) -> Result<Result<String, Refusal>> {
    let mut settings = Vec::new();
    if let Some(model) = &set.model {
        settings.push(Setting::Model {
            model: model.clone(),
        });
    }
    if let Some(every) = set.reflect_every {
        let Ok(turns) = u32::try_from(every) else {
            return Ok(Err(Refusal::Usage {
                words: words::REFLECT_EVERY.into(),
            }));
        };
        settings.push(Setting::ReflectEvery { turns });
    }
    if let Some(allow) = set.allow_net {
        settings.push(Setting::AllowNet { allow });
    }
    if let Some(effort) = set.effort {
        settings.push(Setting::Effort { effort });
    }
    if let Some(newcomers) = set.newcomers {
        settings.push(Setting::Newcomers {
            newcomers: match newcomers {
                protocol::Newcomers::Open => Newcomers::Open,
                protocol::Newcomers::Chief => Newcomers::Chief,
                protocol::Newcomers::Hand => Newcomers::Hand,
            },
        });
    }
    if let Some(text) = &set.character {
        settings.push(Setting::Character { text: text.clone() });
    }
    if let Some(endpoints) = &set.endpoints {
        settings.push(Setting::Endpoints {
            endpoints: endpoints
                .iter()
                .map(|endpoint| Endpoint {
                    name: endpoint.name.trim().to_owned(),
                    base_url: endpoint.base_url.trim().to_owned(),
                })
                .collect(),
        });
    }
    if let Some(workspace) = &set.workspace {
        settings.push(Setting::Workspace {
            workspace: workspace.as_ref().map(|chosen| {
                RemoteWorkspace {
                    url: chosen.url.trim().to_owned(),
                    token: chosen.token.clone(),
                }
            }),
        });
    }
    let act = EditSettings { set: settings };
    Ok(match ask(calls, id, act).await? {
        Done::Done => Ok(words::SETTINGS_SAVED.into()),
        Done::Refused {
            why: why @ SettingRefusal::LongCharacter,
        } => Err(refused(why)),
        Done::Refused { why } => Err(Refusal::Usage {
            words: format!("{}.", capitalised(&why.to_string())),
        }),
    })
}

/// Answers a move of the agents' files to a remote workspace, which
/// has nothing to move until one is chosen.
pub(crate) async fn move_lodges(calls: &Calls) -> Result<Outcome> {
    let state = calls.call(&address::Realm, &ReadState).await?;
    Ok(Outcome::said(Err(match state.settings.workspace.url {
        None => Refusal::Usage {
            words: words::NO_WORKSPACE.into(),
        },
        Some(_) => Refusal::Unsupported {
            what: "`move-lodges` to a remote workspace".into(),
        },
    })))
}

/// The parts of a model's card a catalogue edit sets.
fn card(set: &protocol::EditCatalogueSet) -> Vec<CardSetting> {
    let images =
        set.images.clone().map(|images| CardSetting::Images {
            images: images.map(|takes| ImageLimits {
                max_bytes: takes.max_bytes,
                max_height: takes.max_height,
                max_width: takes.max_width,
                per_line: takes.per_line,
            }),
        });
    let note =
        set.note.clone().map(|note| CardSetting::Note { note });
    let price = set.price.clone().map(|price| CardSetting::Price {
        price: price.map(|price| Price {
            input: price.input,
            output: price.output,
            cache_read: 0.0,
            cache_write: 0.0,
        }),
    });
    images.into_iter().chain(note).chain(price).collect()
}
