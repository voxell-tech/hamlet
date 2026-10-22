//! What the Agent Bazaar asks of the agents: a model added to the
//! catalogue, a skill taken into the library, a hamlet founded from a
//! template, and the packages of the bazaar's server settled into the
//! home and packed from it.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use hamlet_core::agents;
use hamlet_core::catalogue::ModelCatalogue;
use hamlet_core::core::{HomeDir, RefreshLists, Settings};
use hamlet_engine_api::Failure;
use hamlet_engine_api::agents::{
    AgentsReply, Bought, Delivery, Destination, HousedTool,
    ListedModel, PACKAGE_PART, PackageKind, Packing, Settler,
    SettlerSkill, SkillFile, Template,
};
use hamlet_engine_plugins::Disable;
use hamlet_model::catalogue::{self, Catalogue, Listed, Price};
use hamlet_model::market::package::{
    Contents, Kind, MOST_BYTES, Package,
};
use hamlet_rules::library::Library;
use hamlet_rules::market::{self as bazaar, Kept, Target};
use hamlet_rules::realm::{self, Realm};

use crate::Chief;
use crate::answer::MOST_PRICE;

/// The most letters of a skill's name.
const SKILL_NAME_LONGEST: usize = 64;

/// The most files a skill taken may hold.
const MOST_SKILL_FILES: usize = 256;

/// The most bytes of text a skill taken may hold, all its files
/// together.
const MOST_SKILL_BYTES: usize = 4 << 20;

/// The time a package's parts, or a pack, are held after they were
/// last added to or made.
const HELD_FOR: Duration = Duration::from_secs(10 * 60);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Parcels>()
        .add_observer(switched_off)
        .add_systems(Update, let_go);
}

/// The packages on their way in, part by part, and the packs held to
/// be read, each of the plugin that sent or packed it.
#[derive(Resource, Default)]
pub(crate) struct Parcels {
    coming: HashMap<Coming, Parts>,
    packs: HashMap<u64, Pack>,
    last_id: u64,
}

/// A package coming in: the plugin sending it, the listing and the
/// version.
type Coming = (Entity, String, String);

/// The parts of a package come so far.
struct Parts {
    bytes: Vec<u8>,
    next: u32,
    parts: u32,
    last: Instant,
}

/// A package packed, held until it is read.
struct Pack {
    plugin: Entity,
    bytes: Vec<u8>,
    made: Instant,
}

/// Lets go of what a plugin switched off sent or packed.
fn switched_off(off: On<Disable>, mut parcels: ResMut<Parcels>) {
    parcels
        .coming
        .retain(|(plugin, ..), _| *plugin != off.entity);
    parcels.packs.retain(|_, pack| pack.plugin != off.entity);
}

/// Lets go of what has been held too long.
fn let_go(mut parcels: ResMut<Parcels>) {
    let now = Instant::now();
    if parcels.coming.is_empty() && parcels.packs.is_empty() {
        return;
    }
    parcels
        .coming
        .retain(|_, parts| now.duration_since(parts.last) < HELD_FOR);
    parcels
        .packs
        .retain(|_, pack| now.duration_since(pack.made) < HELD_FOR);
}

/// The Agent Bazaar's methods on [`World`].
pub(crate) trait MarketWorldExt {
    /// Adds `model`, listed by the endpoint `endpoint` of the
    /// settings, to the catalogue, allowed, and gives the name it
    /// goes by.
    fn add_model(
        &mut self,
        endpoint: &str,
        model: ListedModel,
    ) -> Result<String, Failure>;

    /// Takes the skill `name`, made of `files`, into the library for
    /// the chief, as brought from `from`.
    fn take_skill(
        &mut self,
        name: &str,
        files: Vec<SkillFile>,
        from: &str,
    ) -> Result<(), Failure>;

    /// Founds `template` for the chief, and brings its agents up.
    fn found_from(
        &mut self,
        template: Template,
    ) -> Result<(), Failure>;

    /// Takes a part of `delivery` from `plugin`, and settles the
    /// package once its last part has come.
    fn settle_part(
        &mut self,
        plugin: Entity,
        delivery: Delivery,
    ) -> Result<AgentsReply, Failure>;

    /// Writes `delivery`, whose package is whole, into the home for
    /// the chief, records it, and brings up the agents it changed.
    /// Gives what came of it in words.
    fn settle(
        &mut self,
        delivery: Delivery,
    ) -> Result<String, Failure>;

    /// What was bought and settled, by listing.
    fn bought(&self) -> BTreeMap<String, Bought>;

    /// The tools of every house of the realm that can be packed.
    fn housed_tools(&self) -> Vec<HousedTool>;

    /// The package of `packing`, as the bazaar's server takes it.
    fn pack(&self, packing: Packing) -> Result<Vec<u8>, Failure>;

    /// Packs `packing` and holds it for `plugin` to read.
    fn hold_pack(
        &mut self,
        plugin: Entity,
        packing: Packing,
    ) -> Result<AgentsReply, Failure>;

    /// [`PACKAGE_PART`] bytes at most of the pack `id` of `plugin`,
    /// from `offset`. The pack is let go once its last byte is read.
    fn packed_part(
        &mut self,
        plugin: Entity,
        id: u64,
        offset: u64,
    ) -> Result<Vec<u8>, Failure>;
}

impl MarketWorldExt for World {
    fn add_model(
        &mut self,
        endpoint: &str,
        model: ListedModel,
    ) -> Result<String, Failure> {
        let at = self
            .resource::<Settings>()
            .config
            .endpoint(endpoint)
            .cloned()
            .ok_or(Failure::NotFound)?;
        let id = model.id.trim();
        if id.is_empty() {
            return Err(Failure::Refused(
                "the model has no id".into(),
            ));
        }
        let fits = |price: Option<f64>| {
            price.is_none_or(|price| {
                price.is_finite()
                    && (0.0..=MOST_PRICE).contains(&price)
            })
        };
        if !fits(model.input) || !fits(model.output) {
            return Err(Failure::Refused(format!(
                "a price is from 0 to {MOST_PRICE}"
            )));
        }
        let listed = Listed {
            id: id.to_string(),
            price: model
                .input
                .zip(model.output)
                .map(|(input, output)| Price { input, output }),
            context: model.context,
        };
        let home = self.resource::<HomeDir>().0.root.clone();
        let mut all = Catalogue::load(&home);
        let added = all.add(
            endpoint,
            &listed,
            catalogue::is_local(&at.base_url),
        );
        let note = model.note.split_whitespace().collect::<Vec<_>>();
        if !note.is_empty() {
            added.note = note.join(" ");
        }
        let name = added.name();
        let saved = all.save(&home);
        self.resource_mut::<ModelCatalogue>().reread();
        saved
            .map(|()| name)
            .map_err(|why| Failure::Failed(format!("{why:#}")))
    }

    fn take_skill(
        &mut self,
        name: &str,
        files: Vec<SkillFile>,
        from: &str,
    ) -> Result<(), Failure> {
        let fits = !name.is_empty()
            && name.len() <= SKILL_NAME_LONGEST
            && name.chars().all(|c| {
                c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || c == '-'
            });
        if !fits {
            return Err(Failure::Refused(format!(
                "a skill's name is 1 to {SKILL_NAME_LONGEST} \
                 lowercase letters, digits and hyphens"
            )));
        }
        let bytes =
            files.iter().map(|file| file.text.len()).sum::<usize>();
        if files.len() > MOST_SKILL_FILES || bytes > MOST_SKILL_BYTES
        {
            return Err(Failure::Refused(format!(
                "a skill holds at most {MOST_SKILL_FILES} files and \
                 {} MB",
                MOST_SKILL_BYTES >> 20
            )));
        }
        let home = self.resource::<HomeDir>().0.root.clone();
        let library = Library::of(&home);
        if library.root().join(name).exists() {
            return Err(Failure::Refused(format!(
                "the library already has a skill named `{name}`"
            )));
        }
        let files = files
            .into_iter()
            .map(|file| (file.path, file.text))
            .collect::<Vec<_>>();
        let by = format!("chief:{}", self.resource::<Chief>().name);
        library
            .give(name, &files, &by, from)
            .map_err(|why| Failure::Failed(format!("{why:#}")))?;
        self.trigger(RefreshLists);
        Ok(())
    }

    fn found_from(
        &mut self,
        template: Template,
    ) -> Result<(), Failure> {
        let home = self.resource::<HomeDir>().0.root.clone();
        let chief = self.resource::<Chief>().name.clone();
        let template = realm::Template {
            name: template.name,
            title: template.title,
            purpose: template.purpose,
            mayor: settler(template.mayor),
            agents: template
                .agents
                .into_iter()
                .map(settler)
                .collect(),
        };
        Realm::new(home)
            .found_from(&template, &chief)
            .map_err(|why| Failure::Failed(format!("{why:#}")))?;
        agents::sync(self);
        Ok(())
    }

    fn settle_part(
        &mut self,
        plugin: Entity,
        mut delivery: Delivery,
    ) -> Result<AgentsReply, Failure> {
        let refused = |why: String| Err(Failure::Refused(why));
        let key = (
            plugin,
            delivery.listing.clone(),
            delivery.version.clone(),
        );
        let mut parcels = self.resource_mut::<Parcels>();
        let earlier = parcels.coming.remove(&key);
        if delivery.package.len() > PACKAGE_PART {
            return refused(format!(
                "a part of a package is at most {} KB",
                PACKAGE_PART >> 10
            ));
        }
        if delivery.part >= delivery.parts {
            return refused(format!(
                "part {} of {} is not a part",
                delivery.part, delivery.parts
            ));
        }
        let mut parts = match earlier {
            // A first part begins the package again.
            _ if delivery.part == 0 => Parts {
                bytes: Vec::new(),
                next: 0,
                parts: delivery.parts,
                last: Instant::now(),
            },
            Some(parts)
                if parts.next == delivery.part
                    && parts.parts == delivery.parts =>
            {
                parts
            }
            _ => {
                return refused(format!(
                    "part {} of {} came out of order",
                    delivery.part, delivery.parts
                ));
            }
        };
        if parts.bytes.len() + delivery.package.len() > MOST_BYTES {
            return refused(format!(
                "a package is at most {} MB",
                MOST_BYTES >> 20
            ));
        }
        parts.bytes.append(&mut delivery.package);
        parts.next += 1;
        parts.last = Instant::now();
        if parts.next < parts.parts {
            parcels.coming.insert(key, parts);
            return Ok(AgentsReply::Done);
        }
        delivery.package = parts.bytes;
        self.settle(delivery).map(AgentsReply::Settled)
    }

    fn settle(
        &mut self,
        delivery: Delivery,
    ) -> Result<String, Failure> {
        if delivery.package.len() > MOST_BYTES {
            return Err(Failure::Refused(format!(
                "a package is at most {} MB",
                MOST_BYTES >> 20
            )));
        }
        let package = Package::read(&delivery.package)
            .map_err(|why| Failure::Refused(format!("{why:#}")))?;
        let home = self.resource::<HomeDir>().0.root.clone();
        let chief = self.resource::<Chief>().name.clone();
        let target = match delivery.to {
            Destination::Realm => Target::Realm,
            Destination::Hamlet(hamlet) => Target::Hamlet(hamlet),
            Destination::House { hamlet, agent } => {
                Target::House { hamlet, agent }
            }
        };
        let (_, words) = bazaar::settle(
            &home,
            bazaar::Delivery {
                id: &delivery.listing,
                title: &delivery.title,
                version: &delivery.version,
                package: &package,
                target: &target,
                by: &chief,
            },
        )
        .map_err(|why| Failure::Failed(format!("{why:#}")))?;
        agents::sync(self);
        self.trigger(RefreshLists);
        Ok(words)
    }

    fn bought(&self) -> BTreeMap<String, Bought> {
        let home = &self.resource::<HomeDir>().0.root;
        Kept::load(home)
            .0
            .into_iter()
            .map(|(listing, keeping)| {
                let bought = Bought {
                    kind: kind(keeping.kind),
                    title: keeping.title,
                    version: keeping.version,
                    names: keeping.names,
                    hamlet: keeping.hamlet,
                    house: keeping.house,
                };
                (listing, bought)
            })
            .collect()
    }

    fn housed_tools(&self) -> Vec<HousedTool> {
        let home = self.resource::<HomeDir>().0.root.clone();
        bazaar::tools(&Realm::new(home))
            .into_iter()
            .map(|tool| HousedTool {
                hamlet: tool.hamlet,
                agent: tool.agent,
                name: tool.name,
            })
            .collect()
    }

    fn pack(&self, packing: Packing) -> Result<Vec<u8>, Failure> {
        let home = self.resource::<HomeDir>().0.root.clone();
        let realm = Realm::new(home);
        let failed =
            |why: anyhow::Error| Failure::Failed(format!("{why:#}"));
        let contents = match packing {
            Packing::Agent {
                hamlet,
                name,
                notes,
            } => {
                let hamlet = realm
                    .hamlet(&hamlet)
                    .map_err(|_| Failure::NotFound)?;
                hamlet.member(&name).ok_or(Failure::NotFound)?;
                Contents::Agent {
                    agent: bazaar::pack_agent(&hamlet, &name, notes)
                        .map_err(failed)?,
                }
            }
            Packing::Hamlet { name, kind, notes } => {
                realm.hamlet(&name).map_err(|_| Failure::NotFound)?;
                Contents::Hamlet {
                    hamlet: bazaar::pack_hamlet(
                        &realm, &name, &kind, notes,
                    )
                    .map_err(failed)?,
                }
            }
            Packing::Tools(chosen) => {
                let housed = bazaar::tools(&realm);
                let dirs = chosen
                    .iter()
                    .map(|want| {
                        housed
                            .iter()
                            .find(|tool| {
                                tool.hamlet == want.hamlet
                                    && tool.agent == want.agent
                                    && tool.name == want.name
                            })
                            .map(|tool| tool.dir.clone())
                            .ok_or(Failure::NotFound)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Contents::Tools {
                    tools: bazaar::pack_tools(&dirs)
                        .map_err(failed)?,
                }
            }
        };
        let package = Package::new(contents);
        package
            .check()
            .and_then(|()| package.bytes())
            .map_err(|why| Failure::Refused(format!("{why:#}")))
    }

    fn hold_pack(
        &mut self,
        plugin: Entity,
        packing: Packing,
    ) -> Result<AgentsReply, Failure> {
        let bytes = self.pack(packing)?;
        let size = bytes.len() as u64;
        let mut parcels = self.resource_mut::<Parcels>();
        parcels.last_id += 1;
        let id = parcels.last_id;
        parcels.packs.insert(
            id,
            Pack {
                plugin,
                bytes,
                made: Instant::now(),
            },
        );
        Ok(AgentsReply::Packed { id, size })
    }

    fn packed_part(
        &mut self,
        plugin: Entity,
        id: u64,
        offset: u64,
    ) -> Result<Vec<u8>, Failure> {
        let mut parcels = self.resource_mut::<Parcels>();
        let pack = parcels
            .packs
            .get(&id)
            .filter(|pack| pack.plugin == plugin)
            .ok_or(Failure::NotFound)?;
        let size = pack.bytes.len();
        let from =
            usize::try_from(offset).unwrap_or(usize::MAX).min(size);
        let to = from.saturating_add(PACKAGE_PART).min(size);
        let part = pack.bytes[from..to].to_vec();
        if to == size {
            parcels.packs.remove(&id);
        }
        Ok(part)
    }
}

fn kind(kind: Kind) -> PackageKind {
    match kind {
        Kind::Agent => PackageKind::Agent,
        Kind::Hamlet => PackageKind::Hamlet,
        Kind::Tools => PackageKind::Tools,
    }
}

fn settler(settler: Settler) -> realm::Settler {
    realm::Settler {
        name: settler.name,
        purpose: settler.purpose,
        character: settler.character,
        notes: settler.notes,
        skills: settler.skills.into_iter().map(skill).collect(),
    }
}

fn skill(skill: SettlerSkill) -> realm::SettlerSkill {
    realm::SettlerSkill {
        name: skill.name,
        description: skill.description,
        body: skill.body,
    }
}
