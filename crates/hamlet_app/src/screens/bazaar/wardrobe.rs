//! The official looks agents and hamlets wear (docs/looks.md): the
//! LOOKS stall's catalogue from the bazaar's server, what the chief
//! owns there, what each agent and hamlet wears as the home's server
//! keeps it, putting a look on or taking it off, and buying one. The
//! [`closet`] downloads the files of what is worn for the realm.

pub(crate) mod closet;
#[cfg(test)]
mod tests;
pub(crate) mod turntable;

use std::cmp::Ordering;
use std::collections::BTreeMap;

use bevy::app::{App, PreUpdate, Update};
use bevy::ecs::name::Name;
use bevy::ecs::query::{Changed, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::{
    any_component_removed, any_match_filter, resource_changed,
};
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::Res;
use bevy::ecs::world::World;
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{
    AgentNumber, HOME, HamletId, Left, LivesIn, Outfit,
};
use hamlet_protocol::client as protocol;
use hamlet_realm::dress::PlayEmote;
use hamlet_realm::wardrobe::{Asset, Category, Target};

use crate::PanelSystems;
use crate::bazaar::errand::Fetched;
use crate::bazaar::market::client::read;
use crate::bazaar::market::deal::{Act, Event, Paid, pay};
use crate::bazaar::market::lot::{Checkout, Page, Purchase};
use crate::bazaar::market::{self, Deal, Market, browse, refused};
use crate::bazaar::shelf::{self, Listing, Stock, now};
use crate::bazaar::{Stall, show, traded};
use crate::roll::Roll;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        shelf::plugin::<Asset>,
        closet::plugin,
        turntable::plugin,
    ))
    .init_resource::<Wardrobe>()
    .init_resource::<Dressed>()
    .init_resource::<AssetDeal>()
    .init_resource::<Fitting>()
    .add_systems(
        PreUpdate,
        gather
            .run_if(
                resource_changed::<Roll>
                    .or_eager(any_match_filter::<Changed<Outfit>>)
                    .or_eager(any_component_removed::<Outfit>),
            )
            .after(crate::roll::turn_roll)
            .in_set(PanelSystems::Bind),
    )
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

/// One that wears looks: an agent's robot, or a hamlet's houses.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Who {
    /// An agent, by its hamlet's id and its name.
    Agent { hamlet: String, name: String },
    /// A hamlet, by its id.
    Hamlet(String),
}

impl Who {
    pub fn target(&self) -> Target {
        match self {
            Who::Agent { .. } => Target::Agent,
            Who::Hamlet(_) => Target::Hamlet,
        }
    }

    fn wearer(&self) -> protocol::Wearer {
        match self {
            Who::Agent { hamlet, name } => protocol::Wearer::Agent {
                agent: protocol::Address {
                    hamlet: hamlet.clone(),
                    agent: name.clone(),
                },
            },
            Who::Hamlet(id) => {
                protocol::Wearer::Hamlet { hamlet: id.clone() }
            }
        }
    }
}

/// An agent or a hamlet that may wear looks, the words it is shown
/// by, and what it wears.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Dresser {
    pub who: Who,
    pub words: String,
    pub looks: protocol::Looks,
}

/// Every living agent, then every hamlet, as the home's server says
/// each is dressed.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Dressed(pub Vec<Dresser>);

impl Dressed {
    /// Those of `target`.
    pub fn of(
        &self,
        target: Target,
    ) -> impl Iterator<Item = &Dresser> {
        self.0.iter().filter(move |one| one.who.target() == target)
    }

    pub fn find(&self, who: &Who) -> Option<&Dresser> {
        self.0.iter().find(|one| one.who == *who)
    }

    /// Who wears the asset `id`, and as what.
    pub fn wearers(&self, id: &str) -> Vec<(&Dresser, Category)> {
        self.0
            .iter()
            .flat_map(|one| {
                Category::ALL
                    .into_iter()
                    .filter(|category| {
                        worn_in(&one.looks, *category)
                            .is_some_and(|worn| asset_of(worn) == id)
                    })
                    .map(move |category| (one, category))
            })
            .collect()
    }

    /// Every `id@version` worn, each once.
    pub fn files(&self) -> Vec<String> {
        let mut all = self
            .0
            .iter()
            .flat_map(|one| {
                Category::ALL.into_iter().filter_map(|category| {
                    worn_in(&one.looks, category)
                })
            })
            .map(String::from)
            .collect::<Vec<_>>();
        all.sort_unstable();
        all.dedup();
        all
    }
}

/// The `id@version` worn in `category`.
pub(crate) fn worn_in(
    looks: &protocol::Looks,
    category: Category,
) -> Option<&str> {
    match category {
        Category::Emote => looks.emote.as_deref(),
        Category::Skin => looks.skin.as_deref(),
        Category::Mesh => looks.mesh.as_deref(),
    }
}

/// The asset's id of an `id@version` worn.
pub(crate) fn asset_of(worn: &str) -> &str {
    worn.split_once('@').map_or(worn, |(id, _)| id)
}

/// The name a file worn is known by: its asset's id and version.
pub(crate) fn file_of(asset: &Asset) -> String {
    format!("{}@{}", asset.id, asset.version)
}

/// What the chief owns of the official looks, and what came of the
/// last thing worn.
#[derive(Resource, Default)]
pub(crate) struct Wardrobe {
    /// The assets the chief owns, those taken off the shelves among
    /// them, by id, as the bazaar last said.
    pub owned: BTreeMap<String, Asset>,
    /// What came of the last thing worn, and whether it went wrong.
    pub said: Option<(String, bool)>,
    /// The chief whose assets were last asked for.
    asked_for: Option<String>,
    /// The edit of looks the home's server is to answer.
    waiting: Option<protocol::RequestId>,
}

impl Wardrobe {
    pub fn owns(&self, id: &str) -> bool {
        self.owned.contains_key(id)
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

    /// The title of the `id@version` worn, or its id when it is not
    /// owned here.
    pub fn title(&self, worn: &str) -> String {
        let id = asset_of(worn);
        self.owned.get(id).map_or_else(
            || id.to_string(),
            |asset| asset.title.clone(),
        )
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

/// The orders the LOOKS stall is shown in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Order {
    /// What wears it, then its kind.
    #[default]
    Kind,
    Cheap,
    Dear,
}

/// What the LOOKS stall shows: its shelf, or the fitting of everyone
/// or of one.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Fitting {
    pub on: bool,
    pub who: Option<Who>,
    /// The place of the one chosen to wear an asset on its page,
    /// among those that may.
    pub wearer: usize,
}

/// The asset being bought, or last so, by its id.
#[derive(Resource, Default)]
pub(crate) struct AssetDeal(pub Option<(String, Deal)>);

/// What a fetch of the bazaar's server is for, at the LOOKS stall.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    Catalogue,
    Owned,
    Purchase(String),
    /// Whether the purchase of the asset was paid: the asset's id,
    /// and the purchase's.
    Paying(String, String),
    /// A part of a file worn: the file's name, and the byte the part
    /// begins at.
    Part(String, u64),
}

/// Takes in the answer to `call`.
pub(crate) fn answered(
    world: &mut World,
    call: Call,
    answer: Fetched,
) {
    match call {
        Call::Catalogue => catalogued(world, answer),
        Call::Owned => owned(world, answer),
        Call::Purchase(id) => {
            step(world, &id, Event::Bought(read::<Checkout>(answer)));
        }
        Call::Paying(id, _) => {
            let paid =
                read::<Purchase>(answer).map(|state| state.status);
            step(world, &id, Event::Paid(paid));
        }
        Call::Part(file, from) => {
            closet::came(world, file, from, answer);
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

fn catalogued(world: &mut World, answer: Fetched) {
    let items = read::<Page<Asset>>(answer)
        .map(|page| page.items)
        .map_err(|why| why.words());
    if let Ok(items) = &items {
        // What the bazaar says is owned, signed in, is owned.
        let mut wardrobe = world.resource_mut::<Wardrobe>();
        for asset in items.iter().filter(|asset| asset.owned) {
            if !wardrobe.owns(&asset.id) {
                wardrobe
                    .owned
                    .insert(asset.id.clone(), asset.clone());
            }
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

fn owned(world: &mut World, answer: Fetched) {
    match read::<Page<Asset>>(answer) {
        Ok(page) => {
            let owned = page
                .items
                .into_iter()
                .map(|asset| (asset.id.clone(), asset))
                .collect::<BTreeMap<_, _>>();
            if world.resource::<Wardrobe>().owned != owned {
                world.resource_mut::<Wardrobe>().owned = owned;
            }
            remark(world);
        }
        // Kept as last known: a token run out is the market's to
        // tell.
        Err(why) => bevy::log::warn!(
            "what is owned at the LOOKS stall did not come: {}",
            why.words()
        ),
    }
}

/// Asks what is owned once a chief signs in, and forgets it once
/// they sign out.
fn signed(world: &mut World) {
    let login = world
        .resource::<Market>()
        .user()
        .map(|user| user.login.clone());
    let signed_in = world.resource::<Market>().signed_in();
    if !signed_in {
        if world.resource::<Wardrobe>().asked_for.is_some()
            || !world.resource::<Wardrobe>().owned.is_empty()
        {
            signed_out(world);
        }
        return;
    }
    if login.is_none()
        || login == world.resource::<Wardrobe>().asked_for
    {
        return;
    }
    world.resource_mut::<Wardrobe>().asked_for = login;
    ask_owned(world);
}

/// Forgets what the chief owned once they sign out, and stops a
/// payment waited on, which was theirs. What is worn stays worn, and
/// the files already downloaded stay in the realm.
fn signed_out(world: &mut World) {
    let mut wardrobe = world.resource_mut::<Wardrobe>();
    wardrobe.asked_for = None;
    wardrobe.owned.clear();
    let mut deal = world.resource_mut::<AssetDeal>();
    if deal.0.as_ref().is_some_and(|(_, deal)| deal.going()) {
        deal.0 = None;
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
                    ask(
                        world,
                        Call::Paying(id.to_string(), purchase),
                    );
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
        remark(world);
    }
    traded(world);
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

/// Has the home's server put the owned asset `id` on `who` as its
/// `category`, or take off what is there for `None`; an emote put on
/// an agent is played.
pub(crate) fn wear(
    world: &mut World,
    who: &Who,
    category: Category,
    id: Option<String>,
) {
    let target = who.target();
    let mut wardrobe = world.resource_mut::<Wardrobe>();
    let worn = match &id {
        None => None,
        Some(id) => match wardrobe.owned.get(id) {
            None => {
                wardrobe.said = Some((
                    "Buy it at the LOOKS stall first.".into(),
                    true,
                ));
                return;
            }
            Some(asset)
                if asset.target != target
                    || asset.category != category =>
            {
                wardrobe.said = Some((
                    format!("{} is not worn there.", asset.title),
                    true,
                ));
                return;
            }
            Some(asset) => Some(file_of(asset)),
        },
    };
    let played = worn.is_some() && category == Category::Emote;
    let mut set = protocol::LooksSet::default();
    *part(&mut set, category) = Some(worn);
    let wearer = who.wearer();
    let request = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Edit(protocol::Edit::Looks {
            id,
            set,
            wearer,
        })
    });
    let mut wardrobe = world.resource_mut::<Wardrobe>();
    wardrobe.waiting = Some(request);
    wardrobe.said = None;
    if played && let Who::Agent { name, .. } = who {
        play(world, name);
    }
}

/// The part of `set` of `category`.
fn part(
    set: &mut protocol::LooksSet,
    category: Category,
) -> &mut Option<Option<String>> {
    match category {
        Category::Emote => &mut set.emote,
        Category::Skin => &mut set.skin,
        Category::Mesh => &mut set.mesh,
    }
}

/// Has the robot of the agent `name` play the emote it wears now, or
/// as soon as it wears one.
pub(crate) fn play(world: &mut World, name: &str) {
    world.trigger(PlayEmote(name.to_string()));
}

/// Takes in what the home's server said of the last edit of looks.
pub(crate) fn heard(world: &mut World, response: &Response) {
    if world.resource::<Wardrobe>().waiting.as_ref()
        != Some(&response.to)
    {
        return;
    }
    let mut wardrobe = world.resource_mut::<Wardrobe>();
    wardrobe.waiting = None;
    if let Some(why) = refused(&response.message) {
        wardrobe.said = Some((why, true));
    }
}

/// Shows what `who` wears, at the LOOKS stall.
pub(crate) fn fit(world: &mut World, who: Who) {
    *world.resource_mut::<Fitting>() = Fitting {
        on: true,
        who: Some(who),
        wearer: 0,
    };
    world.resource_mut::<Wardrobe>().said = None;
    show(world, Stall::Assets);
}

/// Keeps [`Dressed`] as the client's entities say.
fn gather(world: &mut World) {
    let hamlets = {
        let mut query =
            world
                .query::<(&HamletId, Option<&Name>, Option<&Outfit>)>(
                );
        let mut all = query
            .iter(world)
            .map(|(id, name, outfit)| {
                let words = name
                    .map(|name| name.as_str().to_string())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| id.0.clone());
                Dresser {
                    who: Who::Hamlet(id.0.clone()),
                    words,
                    looks: outfit
                        .map(|o| o.0.clone())
                        .unwrap_or_default(),
                }
            })
            .collect::<Vec<_>>();
        all.sort_by(|a, b| {
            home_first(&a.who).cmp(&home_first(&b.who))
        });
        all
    };
    let mut agents = {
        let mut query = world.query_filtered::<
            (&Name, &LivesIn, Option<&Outfit>),
            (With<AgentNumber>, Without<Left>),
        >();
        query
            .iter(world)
            .filter_map(|(name, lives, outfit)| {
                let hamlet =
                    world.get::<HamletId>(lives.0)?.0.clone();
                let name = name.as_str().to_string();
                Some(Dresser {
                    who: Who::Agent {
                        hamlet,
                        name: name.clone(),
                    },
                    words: name,
                    looks: outfit
                        .map(|o| o.0.clone())
                        .unwrap_or_default(),
                })
            })
            .collect::<Vec<_>>()
    };
    agents
        .sort_by(|a, b| home_first(&a.who).cmp(&home_first(&b.who)));
    agents.extend(hamlets);
    if world.resource::<Dressed>().0 != agents {
        world.resource_mut::<Dressed>().0 = agents;
    }
}

/// The order of `who` in a list: those of the first hamlet first,
/// then by hamlet and name.
fn home_first(who: &Who) -> (bool, &str, &str) {
    match who {
        Who::Agent { hamlet, name } => (hamlet != HOME, hamlet, name),
        Who::Hamlet(id) => (id != HOME, id, ""),
    }
}

/// Stocks the LOOKS stall with `catalogue`, as `GET /v1/assets`
/// answers, the assets of `owned` the chief's, as if the bazaar had
/// just answered.
pub(crate) fn stage(
    world: &mut World,
    catalogue: serde_json::Value,
    owned: &[String],
) {
    let items = serde_json::from_value::<Page<Asset>>(catalogue)
        .map(|page| page.items)
        .unwrap_or_default();
    let login = world
        .resource::<Market>()
        .user()
        .map(|user| user.login.clone());
    let mut wardrobe = world.resource_mut::<Wardrobe>();
    wardrobe.asked_for = login;
    wardrobe.owned = items
        .iter()
        .filter(|asset| owned.contains(&asset.id))
        .map(|asset| (asset.id.clone(), asset.clone()))
        .collect();
    let items = marked(world, items);
    let mut stock = world.resource_mut::<Stock<Asset>>();
    stock.ask();
    stock.came(Ok(items), now());
}
