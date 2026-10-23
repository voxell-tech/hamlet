//! What plugins ask of the store, answered. A plugin makes assets of
//! the types it makes and changes only its own. It reads its own
//! types, and another's that its manifest uses at the version the
//! maker makes, once the person allowed `assets.read` of that type.

use bevy::ecs::entity::Entity;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::system::{Local, Query, Res, ResMut};
use hamlet_engine_api::{
    AssetType, Capability, Failure, Reply, Request, TypeName,
};
use hamlet_engine_grants::{
    Decided, FirstUse, Grants, Verdict, Waiting,
};
use hamlet_engine_plugins::{Asked, Deliver, PluginInfo};

use crate::hold::{Change, Holding, tell};
use crate::registry::TypeRegistry;
use crate::store::AssetStore;

/// The longest an asset's name may be, in bytes.
const LONGEST_NAME: usize = 256;

/// What became of a request.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Boxed, as a reply can be far larger than the rest.
    Done(Box<Result<Reply, Failure>>),
    /// The person is to be asked about reading this type first.
    Asking(TypeName),
}

/// What the store needs to answer a plugin, besides the store.
pub struct Rules<'a> {
    pub registry: &'a TypeRegistry,
    pub grants: &'a Grants,
}

impl Rules<'_> {
    /// The answer to `request` of `plugin`, or `None` when it is not
    /// one for the store. `allowed` when the person allowed this
    /// read just now.
    pub fn answer(
        &self,
        store: &mut AssetStore,
        plugin: &PluginInfo,
        request: &Request,
        allowed: bool,
    ) -> Option<Outcome> {
        let done = match request {
            Request::CreateAsset { kind, name, bytes } => {
                self.create(store, plugin, kind, name, bytes)
            }
            Request::ReadAsset(id) | Request::HoldAsset(id) => {
                let Some(info) = store.get(*id).cloned() else {
                    return Some(Outcome::Done(Box::new(Err(
                        Failure::NotFound,
                    ))));
                };
                let hold = matches!(request, Request::HoldAsset(_));
                // A type passes to a plugin whose id comes first, and
                // what its maker before kept is not handed over.
                if self.makes(plugin, &info.kind.name)
                    && info.owner != plugin.id
                {
                    return Some(Outcome::Done(Box::new(Err(
                        Failure::Refused(format!(
                            "asset {} is {}'s",
                            id.0, info.owner
                        )),
                    ))));
                }
                match self.may_read(plugin, &info.kind.name, allowed)
                {
                    Ok(true) if hold => Ok(Reply::Held(info.version)),
                    Ok(true) => store.read(*id).map(Reply::Read),
                    Ok(false) => {
                        return Some(Outcome::Asking(info.kind.name));
                    }
                    Err(why) => Err(why),
                }
            }
            Request::ReleaseAsset(_) => Ok(Reply::Released),
            Request::Call { .. }
            | Request::Subscribe(_)
            | Request::Emit { .. }
            | Request::Fetch(_)
            | Request::Agents(_)
            | Request::Desktop(_) => return None,
            Request::UpdateAsset { id, bytes } => self
                .own(store, plugin, *id)
                .and_then(|()| store.update(*id, bytes))
                .map(Reply::Updated),
            Request::DeleteAsset(id) => self
                .own(store, plugin, *id)
                .and_then(|()| store.delete(*id))
                .map(|()| Reply::Deleted),
            Request::ListAssets(kind) => {
                let kind = match type_name(kind) {
                    Ok(kind) => kind,
                    Err(why) => {
                        return Some(Outcome::Done(Box::new(Err(
                            why,
                        ))));
                    }
                };
                let makes = self.makes(plugin, &kind);
                match self.may_read(plugin, &kind, allowed) {
                    Ok(true) => Ok(Reply::Listed(
                        store
                            .of_type(&kind)
                            .filter(|info| {
                                !makes || info.owner == plugin.id
                            })
                            .cloned()
                            .collect(),
                    )),
                    Ok(false) => return Some(Outcome::Asking(kind)),
                    Err(why) => Err(why),
                }
            }
        };
        Some(Outcome::Done(Box::new(done)))
    }

    fn create(
        &self,
        store: &mut AssetStore,
        plugin: &PluginInfo,
        kind: &str,
        name: &str,
        bytes: &[u8],
    ) -> Result<Reply, Failure> {
        let kind = type_name(kind)?;
        let version = match self.registry.maker(&kind) {
            Some((maker, version)) if *maker == plugin.id => version,
            Some((maker, _)) => {
                return Err(Failure::Refused(format!(
                    "{kind} is made by {maker}"
                )));
            }
            None => {
                return Err(Failure::Refused(format!(
                    "{} does not make {kind}",
                    plugin.id
                )));
            }
        };
        if name.trim().is_empty()
            || name.len() > LONGEST_NAME
            || name.chars().any(char::is_control)
        {
            return Err(Failure::Refused(
                "the name is empty or not readable".into(),
            ));
        }
        store
            .create(
                plugin.id.clone(),
                AssetType {
                    name: kind,
                    version,
                },
                name.into(),
                bytes,
            )
            .map(Reply::Created)
    }

    /// Whether the asset `id` is `plugin`'s own.
    fn own(
        &self,
        store: &AssetStore,
        plugin: &PluginInfo,
        id: hamlet_engine_api::AssetId,
    ) -> Result<(), Failure> {
        let info = store.get(id).ok_or(Failure::NotFound)?;
        if info.owner == plugin.id {
            Ok(())
        } else {
            Err(Failure::Refused(format!(
                "asset {} is {}'s",
                id.0, info.owner
            )))
        }
    }

    /// Whether `plugin` makes `kind` now.
    fn makes(&self, plugin: &PluginInfo, kind: &TypeName) -> bool {
        self.registry
            .maker(kind)
            .is_some_and(|(maker, _)| *maker == plugin.id)
    }

    /// Whether `plugin` may read assets of `kind`: `Ok(false)` when
    /// the person is to be asked first.
    pub fn may_read(
        &self,
        plugin: &PluginInfo,
        kind: &TypeName,
        allowed: bool,
    ) -> Result<bool, Failure> {
        if self.makes(plugin, kind) {
            return Ok(true);
        }
        let maker = self.registry.maker(kind);
        let Some(used) =
            plugin.assets.uses.iter().find(|used| used.name == *kind)
        else {
            return Err(Failure::Refused(format!(
                "{} does not use {kind}",
                plugin.id
            )));
        };
        let Some((maker, version)) = maker else {
            return Err(Failure::Refused(format!(
                "no plugin makes {kind}"
            )));
        };
        if used.version != version {
            return Err(Failure::Mismatch(format!(
                "{} uses {kind} {}, and {maker} makes {version}",
                plugin.id, used.version
            )));
        }
        if allowed {
            return Ok(true);
        }
        match self.grants.verdict(
            plugin,
            Capability::AssetRead,
            kind.as_str(),
        ) {
            Verdict::Allowed => Ok(true),
            Verdict::Asking => Ok(false),
            Verdict::Refused => Err(Failure::Refused(format!(
                "{} {kind} is not allowed",
                Capability::AssetRead.name()
            ))),
        }
    }
}

fn type_name(kind: &str) -> Result<TypeName, Failure> {
    TypeName::try_from(kind)
        .map_err(|why| Failure::Refused(why.to_string()))
}

/// Answers what plugins asked of the store, and what waited for the
/// person.
pub(crate) fn answer(
    mut asked: MessageReader<Asked>,
    mut decided: MessageReader<Decided>,
    mut plugins: Query<(Entity, &PluginInfo, &mut Holding)>,
    mut store: ResMut<AssetStore>,
    registry: Res<TypeRegistry>,
    grants: Res<Grants>,
    mut parked: Local<Waiting<Asked>>,
    mut deliver: MessageWriter<Deliver>,
    mut first_use: MessageWriter<FirstUse>,
) {
    parked.keep(|plugin| plugins.contains(plugin));
    let mut again = Vec::new();
    for (waiting, allowed) in parked.settle(decided.read()) {
        if allowed {
            again.push((waiting, true));
        } else {
            deliver.write(waiting.reply(Err(Failure::Refused(
                format!(
                    "{} was not allowed",
                    Capability::AssetRead.name()
                ),
            ))));
        }
    }
    let rules = Rules {
        registry: &registry,
        grants: &grants,
    };
    let fresh = asked.read().map(|asked| (asked.clone(), false));
    let mut changes = Vec::<Change>::new();
    for (request, allowed) in again.into_iter().chain(fresh) {
        let Ok((_, plugin, mut holding)) =
            plugins.get_mut(request.plugin)
        else {
            continue;
        };
        match rules.answer(
            &mut store,
            plugin,
            &request.request,
            allowed,
        ) {
            Some(Outcome::Done(reply)) => {
                let reply = *reply;
                let depth = request.depth;
                match (&request.request, &reply) {
                    (Request::HoldAsset(id), Ok(_)) => {
                        holding.0.insert(*id);
                    }
                    (Request::ReleaseAsset(id), _) => {
                        holding.0.remove(id);
                    }
                    (
                        Request::UpdateAsset { id, .. },
                        Ok(Reply::Updated(version)),
                    ) => changes.push((*id, Some(*version), depth)),
                    (Request::DeleteAsset(id), Ok(_)) => {
                        changes.push((*id, None, depth));
                    }
                    _ => {}
                }
                deliver.write(request.reply(reply));
            }
            Some(Outcome::Asking(kind)) => {
                parked.park(
                    request.plugin,
                    Capability::AssetRead,
                    kind.to_string(),
                    request,
                    &mut first_use,
                );
            }
            None => {}
        }
    }
    if !changes.is_empty() {
        tell(
            &changes,
            plugins
                .iter_mut()
                .map(|(plugin, _, holding)| (plugin, holding)),
            &mut deliver,
        );
    }
}
