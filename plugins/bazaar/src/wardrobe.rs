//! The official assets agents and hamlets wear (docs/looks.md): the
//! Asset stall's catalogue from the bazaar's server, what the chief
//! owns, what each agent and hamlet wears, and buying an asset. The
//! [`closet`] keeps what is owned and worn in the plugin's asset
//! store, and puts the files of what is worn there for the realm.

pub(crate) mod closet;
pub(crate) mod turntable;

use std::cmp::Ordering;
use std::collections::BTreeMap;

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::{Failure, Reply, encode, warn};
use looks::wardrobe::{Asset, Category, Target};
use looks::{Dress, Outfit, Worn};
use serde::{Deserialize, Serialize};

use crate::market::client::read;
use crate::market::lot::{Checkout, Page, Purchase};
use crate::market::{
    self, Act, Deal, Event, Market, Paid, browse, pay,
};
use crate::shelf::{Listing, Stock, now};
use crate::{BazaarWorldExt, Stall};

/// The event that has an agent's robot play the emote it wears now,
/// by its short name: [`looks::PLAY`].
const PLAY: &str = "emote";

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        crate::shelf::plugin::<Asset>,
        closet::plugin,
        turntable::plugin,
    ))
    .init_resource::<Wardrobe>()
    .init_resource::<AssetDeal>()
    .init_saved_resource::<Fitting>()
    .add_systems(
        Update,
        (
            signed.run_if(resource_changed::<Market>),
            tick.run_if(|deal: Res<AssetDeal>| {
                matches!(deal.0, Some((_, Deal::Paying { .. })))
            }),
        ),
    );
}

/// What every agent and hamlet wears, by name: each [`Outfit`] holds
/// the ids of the assets chosen, owned or not.
#[derive(
    Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub(crate) struct Wearing {
    pub agents: BTreeMap<String, Outfit>,
    /// By the hamlet's name on disk.
    pub hamlets: BTreeMap<String, Outfit>,
}

impl Wearing {
    fn all(&self, target: Target) -> &BTreeMap<String, Outfit> {
        match target {
            Target::Agent => &self.agents,
            Target::Hamlet => &self.hamlets,
        }
    }

    /// What `name` of `target` wears.
    pub fn of(&self, target: Target, name: &str) -> Option<&Outfit> {
        self.all(target).get(name)
    }

    /// Puts `id` on `name` of `target` in `category`, or takes off
    /// what is there for `None`.
    pub fn wear(
        &mut self,
        target: Target,
        name: &str,
        category: Category,
        id: Option<String>,
    ) {
        let all = match target {
            Target::Agent => &mut self.agents,
            Target::Hamlet => &mut self.hamlets,
        };
        let outfit = all.entry(name.to_string()).or_default();
        *slot(outfit, category) = id;
        if *outfit == Outfit::default() {
            all.remove(name);
        }
    }

    /// Who wears the asset `id`, and in what.
    pub fn wearers(
        &self,
        id: &str,
    ) -> Vec<(Target, String, Category)> {
        let mut wearers = Vec::new();
        for target in Target::ALL {
            for (name, outfit) in self.all(target) {
                for category in Category::ALL {
                    if worn_in(outfit, category) == Some(id) {
                        wearers.push((
                            target,
                            name.clone(),
                            category,
                        ));
                    }
                }
            }
        }
        wearers
    }

    /// Every asset worn by anyone.
    fn ids(&self) -> impl Iterator<Item = &str> {
        self.agents
            .values()
            .chain(self.hamlets.values())
            .flat_map(|outfit| {
                [&outfit.skin, &outfit.mesh, &outfit.emote]
            })
            .flatten()
            .map(String::as_str)
    }
}

/// The id worn in `category`.
pub(crate) fn worn_in(
    outfit: &Outfit,
    category: Category,
) -> Option<&str> {
    match category {
        Category::Emote => outfit.emote.as_deref(),
        Category::Skin => outfit.skin.as_deref(),
        Category::Mesh => outfit.mesh.as_deref(),
    }
}

fn slot(
    outfit: &mut Outfit,
    category: Category,
) -> &mut Option<String> {
    match category {
        Category::Emote => &mut outfit.emote,
        Category::Skin => &mut outfit.skin,
        Category::Mesh => &mut outfit.mesh,
    }
}

/// What the chief owns of the official assets, and what is worn.
#[derive(Resource, Default)]
pub(crate) struct Wardrobe {
    /// The assets the chief owns, those taken off the shelves among
    /// them, by id, as the bazaar last said.
    pub owned: BTreeMap<String, Asset>,
    pub wearing: Wearing,
    /// What came of the last thing worn, and whether it went wrong.
    pub said: Option<(String, bool)>,
    /// What was kept was read from the store, so what is worn may be
    /// written.
    pub read: bool,
    /// The chief whose assets were last asked for.
    asked_for: Option<String>,
}

/// What the closet keeps of the wardrobe for the next run.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub(crate) struct Kept {
    pub wearing: Wearing,
    pub owned: Vec<Asset>,
}

impl Wardrobe {
    pub fn owns(&self, id: &str) -> bool {
        self.owned.contains_key(id)
    }

    /// The name of the file of the owned asset `id`, at the version
    /// owned.
    pub fn file(&self, id: &str) -> Option<String> {
        let asset = self.owned.get(id)?;
        Some(Worn::file(&asset.id, &asset.version))
    }

    /// The assets the chief owns that `target` wears in `category`.
    pub fn wearable(
        &self,
        target: Target,
        category: Category,
    ) -> impl Iterator<Item = &Asset> {
        self.owned.values().filter(move |asset| {
            asset.target == target && asset.category == category
        })
    }

    /// The files of what is worn and owned, each with its asset's
    /// id.
    pub fn wanted(&self) -> BTreeMap<String, String> {
        self.wearing
            .ids()
            .filter_map(|id| Some((self.file(id)?, id.to_string())))
            .collect()
    }

    /// What the realm is told is worn: each owned asset worn whose
    /// file is whole in the store, `stored` holding the parts of each
    /// file whole there.
    pub fn worn(&self, stored: &BTreeMap<String, u32>) -> Worn {
        let ready = |id: &Option<String>| {
            id.as_deref()
                .and_then(|id| self.file(id))
                .filter(|file| stored.contains_key(file))
        };
        let dressed = |all: &BTreeMap<String, Outfit>| {
            all.iter()
                .map(|(name, outfit)| {
                    let outfit = Outfit {
                        skin: ready(&outfit.skin),
                        mesh: ready(&outfit.mesh),
                        emote: ready(&outfit.emote),
                    };
                    (name.clone(), outfit)
                })
                .filter(|(_, outfit)| *outfit != Outfit::default())
                .collect::<BTreeMap<_, _>>()
        };
        let mut worn = Worn {
            agents: dressed(&self.wearing.agents),
            hamlets: dressed(&self.wearing.hamlets),
            parts: BTreeMap::new(),
        };
        worn.parts = worn
            .files()
            .filter_map(|file| {
                Some((file.to_string(), *stored.get(file)?))
            })
            .collect();
        worn
    }

    pub fn kept(&self) -> Kept {
        Kept {
            wearing: self.wearing.clone(),
            owned: self.owned.values().cloned().collect(),
        }
    }
}

impl Listing for Asset {
    type Id = String;
    /// What wears it and its kind.
    type Filter = (Option<Target>, Option<Category>);
    type Order = Order;

    const ORDERS: &'static [(Order, &'static str)] = &[
        (Order::Kind, "By kind"),
        (Order::Cheap, "Price, lowest"),
        (Order::Dear, "Price, highest"),
    ];

    fn id(&self) -> String {
        self.id.clone()
    }

    fn told(&self) -> String {
        format!(
            "{} {} {} {}",
            self.title,
            self.summary,
            self.category.one(),
            self.target.name()
        )
    }

    fn admits(&self, (target, category): &Self::Filter) -> bool {
        target.is_none_or(|t| self.target == t)
            && category.is_none_or(|c| self.category == c)
    }

    fn order(&self, other: &Asset, order: Order) -> Ordering {
        let kind = |asset: &Asset| (asset.target, asset.category);
        match order {
            Order::Kind => kind(self).cmp(&kind(other)),
            Order::Cheap => self.price_cents.cmp(&other.price_cents),
            Order::Dear => other.price_cents.cmp(&self.price_cents),
        }
        .then_with(|| self.title.cmp(&other.title))
    }
}

/// The orders the Asset stall is shown in.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub(crate) enum Order {
    /// What wears it, then its kind.
    #[default]
    Kind,
    Cheap,
    Dear,
}

/// What the Asset stall shows: its shelf, or what each agent and
/// hamlet wears, kept across a reload.
#[derive(
    Resource, Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub(crate) struct Fitting {
    pub on: bool,
    /// The agent or hamlet whose looks are shown, by its name.
    pub who: Option<(Target, String)>,
    /// The place of the one chosen to wear an asset on its page,
    /// among those that may.
    pub wearer: usize,
}

/// The asset being bought, or last so, by its id.
#[derive(Resource, Default)]
pub(crate) struct AssetDeal(pub Option<(String, Deal)>);

/// What a fetch of the bazaar's server is for, at the Asset stall.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    Catalogue,
    Owned,
    Purchase(String),
    /// Whether the purchase of the asset was paid: the asset's id,
    /// and the purchase's.
    Paying(String, String),
    /// A part of a file worn, by the file's name and the part's
    /// place.
    Part(String, u32),
}

/// Takes in the answer to `call`.
pub(crate) fn answered(
    world: &mut World,
    call: Call,
    reply: Result<Reply, Failure>,
) {
    match call {
        Call::Catalogue => catalogued(world, reply),
        Call::Owned => owned(world, reply),
        Call::Purchase(id) => {
            step(world, &id, Event::Bought(read::<Checkout>(reply)))
        }
        Call::Paying(id, _) => {
            let paid =
                read::<Purchase>(reply).map(|state| state.status);
            step(world, &id, Event::Paid(paid))
        }
        Call::Part(file, part) => {
            closet::came(world, file, part, reply)
        }
    }
}

fn ask(world: &mut World, call: Call) {
    let server = &world.resource::<Market>().server;
    let request = match &call {
        Call::Catalogue => server.assets(),
        Call::Owned => server.my_assets(),
        Call::Purchase(id) => server.asset_purchase(id),
        Call::Paying(_, purchase) => {
            server.asset_purchase_state(purchase)
        }
        Call::Part(..) => return,
    };
    market::send(world, market::Call::Wardrobe(call), request);
}

/// Asks for the catalogue, unless it is being asked for.
pub(crate) fn ask_catalogue(world: &mut World) {
    if world.resource::<Stock<Asset>>().asking {
        return;
    }
    world.resource_mut::<Stock<Asset>>().ask();
    ask(world, Call::Catalogue);
}

/// Asks for the catalogue the first time the stall is shown.
pub(crate) fn stocked(world: &mut World) {
    if !world.resource::<Stock<Asset>>().asked {
        ask_catalogue(world);
    }
}

fn catalogued(world: &mut World, reply: Result<Reply, Failure>) {
    let items = read::<Page<Asset>>(reply)
        .map(|page| page.items)
        .map_err(|why| why.words());
    if let Ok(items) = &items {
        // What the bazaar says is owned, signed in, is owned.
        let mut wardrobe = world.resource_mut::<Wardrobe>();
        let mut more = false;
        for asset in items.iter().filter(|asset| asset.owned) {
            if !wardrobe.owns(&asset.id) {
                wardrobe
                    .owned
                    .insert(asset.id.clone(), asset.clone());
                more = true;
            }
        }
        if more {
            closet::keep(world);
        }
    }
    let items = items.map(|items| marked(world, items));
    world.resource_mut::<Stock<Asset>>().came(items, now());
}

/// `items` each marked owned when the chief owns it.
fn marked(world: &World, mut items: Vec<Asset>) -> Vec<Asset> {
    let wardrobe = world.resource::<Wardrobe>();
    for asset in &mut items {
        asset.owned = wardrobe.owns(&asset.id);
    }
    items
}

/// Marks the catalogue's assets owned as the wardrobe says.
fn remark(world: &mut World) {
    let items = world.resource::<Stock<Asset>>().items.clone();
    let items = marked(world, items);
    if items != world.resource::<Stock<Asset>>().items {
        world.resource_mut::<Stock<Asset>>().stock(items);
    }
}

/// Asks what the one signed in owns.
fn ask_owned(world: &mut World) {
    if world.resource::<Market>().signed_in() {
        ask(world, Call::Owned);
    }
}

fn owned(world: &mut World, reply: Result<Reply, Failure>) {
    match read::<Page<Asset>>(reply) {
        Ok(page) => {
            let owned = page
                .items
                .into_iter()
                .map(|asset| (asset.id.clone(), asset))
                .collect::<BTreeMap<_, _>>();
            if world.resource::<Wardrobe>().owned != owned {
                world.resource_mut::<Wardrobe>().owned = owned;
                closet::keep(world);
            }
            remark(world);
        }
        // Kept as last known: a token run out is the market's to
        // tell.
        Err(why) => warn(&format!(
            "what is owned at the Asset stall did not come: {}",
            why.words()
        )),
    }
}

/// Asks what is owned once a chief signs in.
fn signed(world: &mut World) {
    let login = world
        .resource::<Market>()
        .user()
        .map(|user| user.login.clone());
    if login.is_none()
        || login == world.resource::<Wardrobe>().asked_for
    {
        return;
    }
    world.resource_mut::<Wardrobe>().asked_for = login;
    ask_owned(world);
}

/// Forgets what the chief owned once they sign out, so nothing of it
/// is worn, and stops a payment waited on, which was theirs.
pub(crate) fn signed_out(world: &mut World) {
    let mut wardrobe = world.resource_mut::<Wardrobe>();
    wardrobe.asked_for = None;
    let had = !wardrobe.owned.is_empty();
    wardrobe.owned.clear();
    let mut deal = world.resource_mut::<AssetDeal>();
    if deal.0.as_ref().is_some_and(|(_, deal)| deal.going()) {
        deal.0 = None;
    }
    if had {
        closet::keep(world);
    }
    remark(world);
}

/// Buys the asset `id`, or claims it when it is free. A purchase
/// asked while another waits on Stripe takes its place; the one left
/// is still the chief's if it is paid after all, once what is owned
/// is asked again.
pub(crate) fn buy(world: &mut World, id: &str) {
    let deal = world.resource::<AssetDeal>();
    if deal
        .0
        .as_ref()
        .is_some_and(|(dealt, deal)| dealt == id && deal.going())
    {
        return;
    }
    let deal = match world.resource::<Market>().signed_in() {
        false => Deal::Failed("Sign in with GitHub to buy.".into()),
        true => {
            ask(world, Call::Purchase(id.to_string()));
            Deal::Buying
        }
    };
    world.resource_mut::<AssetDeal>().0 =
        Some((id.to_string(), deal));
}

/// Moves the purchase of the asset `id` by `event`, if it is the one
/// going on, and does what it asks.
fn step(world: &mut World, id: &str, event: Event) {
    let Some((dealing, deal)) =
        world.resource::<AssetDeal>().0.clone()
    else {
        return;
    };
    if dealing != id || !deal.going() {
        return;
    }
    let deal = match pay(deal, event, now()) {
        Paid::Owned => {
            bought(world, id);
            Deal::Done("It is yours.".into())
        }
        Paid::Going(deal, act) => {
            match act {
                Some(Act::Open(url)) => browse(world, &url),
                Some(Act::Poll(purchase)) => {
                    ask(world, Call::Paying(id.to_string(), purchase))
                }
                _ => {}
            }
            deal
        }
    };
    world.resource_mut::<AssetDeal>().0 =
        Some((id.to_string(), deal));
}

/// Takes the asset `id` as the chief's, cheers it in the realm, and
/// asks again what is owned.
fn bought(world: &mut World, id: &str) {
    let asset = world
        .resource::<Stock<Asset>>()
        .get(&id.to_string())
        .cloned();
    if let Some(mut asset) = asset {
        asset.owned = true;
        world
            .resource_mut::<Wardrobe>()
            .owned
            .insert(id.to_string(), asset);
        closet::keep(world);
        remark(world);
    }
    world.traded(Stall::Assets);
    ask_owned(world);
}

fn tick(world: &mut World) {
    let now = now();
    let due = match &world.resource::<AssetDeal>().0 {
        Some((id, Deal::Paying { asking, .. }))
            if asking.over(now) || asking.due(now) =>
        {
            Some(id.clone())
        }
        _ => None,
    };
    if let Some(id) = due {
        step(world, &id, Event::Tick(now));
    }
}

/// Puts the asset `id` on `name` of `target` in `category`, or takes
/// off what is there for `None`; an emote put on an agent is played.
pub(crate) fn wear(
    world: &mut World,
    (target, name): (Target, &str),
    category: Category,
    id: Option<String>,
) {
    let mut wardrobe = world.resource_mut::<Wardrobe>();
    if let Some(id) = &id {
        let fits = wardrobe.owned.get(id).map(|asset| {
            (
                asset.target == target && asset.category == category,
                asset.title.clone(),
            )
        });
        let refused = match fits {
            None => {
                Some("Buy it at the Asset stall first.".to_string())
            }
            Some((false, title)) => {
                Some(format!("{title} is not worn there."))
            }
            Some((true, _)) => None,
        };
        if let Some(why) = refused {
            wardrobe.said = Some((why, true));
            return;
        }
    }
    let file = id.as_deref().and_then(|id| wardrobe.file(id));
    wardrobe.wearing.wear(target, name, category, id);
    wardrobe.said = None;
    if let Some(file) = file {
        // Worn again, a file that did not come is asked for at once.
        closet::retry(world, &file);
        if target == Target::Agent && category == Category::Emote {
            play(world, name);
        }
    }
    closet::keep(world);
}

/// Has the agent `name` play the emote it wears, now, or once the
/// realm can.
pub(crate) fn play(world: &mut World, name: &str) {
    closet::play(world, name);
}

/// Has the realm play the emote of the agent `name`.
pub(crate) fn emit_play(world: &mut World, name: &str) {
    match encode(&name.to_string()) {
        Ok(payload) => {
            world.emit(PLAY, &payload);
        }
        Err(why) => warn(&format!("emote {name}: {why}")),
    }
}

/// Shows what `dress` names wears, at the Asset stall.
pub(crate) fn dress(world: &mut World, dress: Dress) {
    *world.resource_mut::<Fitting>() = Fitting {
        on: true,
        who: Some((dress.target, dress.name)),
        wearer: 0,
    };
    world.show(Stall::Assets);
}

#[cfg(test)]
mod tests;
