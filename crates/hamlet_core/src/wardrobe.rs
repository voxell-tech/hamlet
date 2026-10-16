//! The official assets as a resource: the Asset stall's catalogue,
//! what the chief owns, what each agent and hamlet wears, and the
//! files of what is worn, downloaded and kept in memory only.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use bevy_app::{App, Startup, Update};
use bevy_ecs::prelude::*;
use hamlet_model::market::client::{Client, refused, signed_out};
use hamlet_model::market::lot::Paying;
use hamlet_model::wardrobe::{
    Asset, Category, Emote, Looks, Skin, Target,
};
use tokio::task::AbortHandle;

use crate::core::{HomeDir, Model, Rounds, Tokio, answered, browse};
use crate::market::{Account, Deal, Market};

/// The time before a file that could not be downloaded is asked for
/// again.
const RETRY: Duration = Duration::from_secs(30);

/// The time between asks whether a payment went through.
const PAYMENT_POLL: Duration = Duration::from_secs(3);

/// The longest a payment is waited for.
const PAYMENT_PATIENCE: Duration = Duration::from_secs(30 * 60);

/// Why something was refused, in words for the chief.
type Why = String;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Wardrobe>()
        .init_resource::<Rounds<Listed>>()
        .init_resource::<Rounds<Fetched>>()
        .init_resource::<Rounds<Dealt>>()
        .add_observer(ask)
        .add_observer(buy)
        .add_observer(wear)
        // The market reads the server and the token first.
        .add_systems(Startup, open.after(crate::market::open))
        .add_systems(
            Update,
            (
                signed_in,
                listed.run_if(answered::<Listed>),
                fetched.run_if(answered::<Fetched>),
                dealt.run_if(answered::<Dealt>),
                fetch,
            )
                .chain(),
        );
}

/// The official assets and what is worn of them.
#[derive(Resource, Default)]
pub struct Wardrobe {
    /// Every asset on the stall's shelves.
    pub catalog: Vec<Asset>,
    /// The assets the chief owns, the ones taken off the shelves
    /// among them, by id.
    pub owned: BTreeMap<String, Asset>,
    /// The stall is being asked.
    pub asking: bool,
    pub problem: Option<Why>,
    /// What every agent and hamlet wears, kept in `looks.json`.
    pub looks: Looks,
    /// The files of the assets worn, by id, in memory only.
    pub files: BTreeMap<String, File>,
    /// The asset being bought, or last so, by its id.
    pub deal: Option<(String, Deal)>,
    /// What came of the last thing worn, and whether it went wrong.
    pub said: Option<(String, bool)>,
    /// The chief whose assets were last asked for.
    asked_for: Option<String>,
    /// Files are being downloaded.
    fetching: bool,
    /// When each file that could not be downloaded was last asked
    /// for, by asset id.
    missed: BTreeMap<String, std::time::Instant>,
    paying_task: Option<AbortHandle>,
}

/// The file of an asset, as it was read.
#[derive(Clone, Debug, PartialEq)]
pub enum File {
    Skin(Skin),
    Emote(Emote),
    /// A `.glb`.
    Mesh(Vec<u8>),
    Failed(Why),
}

impl Wardrobe {
    /// The asset `id`, owned or on the shelves.
    pub fn asset(&self, id: &str) -> Option<&Asset> {
        self.owned.get(id).or_else(|| {
            self.catalog.iter().find(|asset| asset.id == id)
        })
    }

    pub fn owns(&self, id: &str) -> bool {
        self.owned.contains_key(id)
    }

    /// The assets the chief owns that `target` can wear in
    /// `category`.
    pub fn wearable(
        &self,
        target: Target,
        category: Category,
    ) -> impl Iterator<Item = &Asset> {
        self.owned.values().filter(move |asset| {
            asset.target == target && asset.category == category
        })
    }

    /// The file of what `name` of `target` wears in `category`, once
    /// it is downloaded.
    pub fn worn(
        &self,
        target: Target,
        name: &str,
        category: Category,
    ) -> Option<(&str, &File)> {
        let id = self.looks.of(target, name)?.get(category)?;
        // Only what is owned is worn: a file kept from before a
        // sign-out stays unworn.
        if !self.owns(id) {
            return None;
        }
        self.files.get(id).map(|file| (id, file))
    }

    /// True while the asset `id` is being bought.
    pub fn dealing(&self) -> bool {
        matches!(
            self.deal,
            Some((
                _,
                Deal::Buying | Deal::Paying(_) | Deal::Fetching
            ))
        )
    }
}

/// Asks the stall for its catalogue and what the chief owns.
#[derive(Event)]
pub struct AskWardrobe;

/// Buys the asset `id`, or claims it when it is free.
#[derive(Event, Clone)]
pub struct BuyAsset(pub String);

/// Puts the asset `id` on `name` of `target` in `category`, or takes
/// off what is there for `None`.
#[derive(Event, Clone)]
pub struct Wear {
    pub target: Target,
    pub name: String,
    pub category: Category,
    pub id: Option<String>,
}

/// Has the agent `name` play the emote it wears, now.
#[derive(Event, Clone)]
pub struct PlayEmote(pub String);

/// The catalogue, and what the chief owns when signed in.
type Listed = Result<(Vec<Asset>, Option<Vec<Asset>>), Why>;

/// An asset's file, read, by its id.
type Fetched = (String, File);

/// A purchase's progress, by asset id.
type Dealt = (String, Deal);

/// Reads what is worn, and asks the stall for its shelves.
fn open(world: &mut World) {
    let home = world.resource::<HomeDir>().0.root.clone();
    world.resource_mut::<Wardrobe>().looks = Looks::load(&home);
    // A recorded session asks nothing of a network.
    if !world.resource::<Model>().replay {
        world.trigger(AskWardrobe);
    }
}

/// Asks again for what is owned once a chief signs in, and forgets
/// it once they sign out.
fn signed_in(
    market: Res<Market>,
    mut wardrobe: ResMut<Wardrobe>,
    mut commands: Commands,
) {
    if !market.is_changed() {
        return;
    }
    let now = market.user().map(|user| user.id.clone());
    if now == wardrobe.asked_for {
        return;
    }
    match (&now, &market.account) {
        (Some(_), _) => commands.trigger(AskWardrobe),
        (None, Account::Out) => {
            // A payment waited on is the last chief's.
            if let Some(task) = wardrobe.paying_task.take() {
                task.abort();
            }
            if wardrobe.dealing() {
                wardrobe.deal = None;
            }
            wardrobe.owned.clear();
            wardrobe.files.clear();
            wardrobe.missed.clear();
            for asset in &mut wardrobe.catalog {
                asset.owned = false;
            }
        }
        _ => return,
    }
    wardrobe.asked_for = now;
}

fn ask(
    _: On<AskWardrobe>,
    tokio: Res<Tokio>,
    market: Res<Market>,
    mut rounds: ResMut<Rounds<Listed>>,
    mut wardrobe: ResMut<Wardrobe>,
) {
    wardrobe.asking = true;
    let client = market.client();
    let signed = market.signed();
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        let asked = async {
            let catalog = client.assets().await?;
            // A token that ran out leaves the shelves to be shown, as
            // to anyone signed out.
            let owned = match signed {
                true => client.my_assets().await.ok(),
                false => None,
            };
            anyhow::Ok((catalog, owned))
        };
        tell(asked.await.map_err(|e| format!("{e:#}")));
    });
}

fn listed(
    mut rounds: ResMut<Rounds<Listed>>,
    mut wardrobe: ResMut<Wardrobe>,
) {
    while let Some(asked) = rounds.next() {
        wardrobe.asking = false;
        match asked {
            Ok((catalog, owned)) => {
                wardrobe.problem = None;
                wardrobe.catalog = catalog;
                if let Some(owned) = owned {
                    wardrobe.owned = owned
                        .into_iter()
                        .map(|asset| (asset.id.clone(), asset))
                        .collect();
                }
                let owned = wardrobe
                    .owned
                    .keys()
                    .cloned()
                    .collect::<BTreeSet<_>>();
                for asset in &mut wardrobe.catalog {
                    asset.owned |= owned.contains(&asset.id);
                }
            }
            Err(why) => wardrobe.problem = Some(why),
        }
    }
}

/// Downloads the files of what is worn and owned and not yet in
/// memory, one batch at a time.
fn fetch(
    tokio: Res<Tokio>,
    market: Res<Market>,
    mut rounds: ResMut<Rounds<Fetched>>,
    mut wardrobe: ResMut<Wardrobe>,
) {
    if wardrobe.fetching || !market.signed() {
        return;
    }
    let now = std::time::Instant::now();
    let wanted = wanted(&wardrobe, now);
    if wanted.is_empty() {
        return;
    }
    wardrobe.fetching = true;
    for (id, ..) in &wanted {
        wardrobe.missed.insert(id.clone(), now);
    }
    let client = market.client();
    let tell = rounds.begin();
    tokio.0.spawn(async move {
        for (id, target, category) in wanted {
            let file = match client.asset_file(&id).await {
                Ok(bytes) => read(&bytes, target, category),
                Err(e) => File::Failed(format!("{e:#}")),
            };
            tell((id, file));
        }
        tell((String::new(), File::Failed(String::new())));
    });
}

/// The assets worn and owned whose files are to be downloaded at
/// `now`: those not in memory, a file that could not be downloaded
/// once [`RETRY`] has passed.
fn wanted(
    wardrobe: &Wardrobe,
    now: std::time::Instant,
) -> BTreeSet<(String, Target, Category)> {
    wardrobe
        .looks
        .worn()
        .filter(|id| {
            !matches!(
                wardrobe.files.get(*id),
                Some(File::Skin(_) | File::Emote(_) | File::Mesh(_))
            )
        })
        .filter(|id| {
            wardrobe
                .missed
                .get(*id)
                .is_none_or(|at| now.duration_since(*at) >= RETRY)
        })
        .filter_map(|id| wardrobe.owned.get(id))
        .map(|asset| (asset.id.clone(), asset.target, asset.category))
        .collect()
}

/// What the file of an asset of `target` and `category` holds.
fn read(bytes: &[u8], target: Target, category: Category) -> File {
    let read = match category {
        Category::Skin => Skin::read(bytes, target).map(File::Skin),
        Category::Emote => {
            Emote::read(bytes, target).map(File::Emote)
        }
        Category::Mesh => Ok(File::Mesh(bytes.to_vec())),
    };
    read.unwrap_or_else(|e| File::Failed(format!("{e:#}")))
}

fn fetched(
    mut rounds: ResMut<Rounds<Fetched>>,
    mut wardrobe: ResMut<Wardrobe>,
) {
    while let Some((id, file)) = rounds.next() {
        // The end of a batch.
        if id.is_empty() {
            wardrobe.fetching = false;
            continue;
        }
        if !matches!(file, File::Failed(_)) {
            wardrobe.missed.remove(&id);
        }
        wardrobe.files.insert(id, file);
    }
}

fn wear(
    asked: On<Wear>,
    home: Res<HomeDir>,
    mut wardrobe: ResMut<Wardrobe>,
) {
    let Wear {
        target,
        name,
        category,
        id,
    } = asked.event().clone();
    if let Some(id) = &id {
        let fits = wardrobe.owned.get(id).map(|asset| {
            (
                asset.target == target && asset.category == category,
                asset.title.clone(),
            )
        });
        match fits {
            None => {
                wardrobe.said = Some((
                    "Buy it at the Asset stall first.".into(),
                    true,
                ));
                return;
            }
            Some((false, title)) => {
                wardrobe.said = Some((
                    format!("{title} is not worn there."),
                    true,
                ));
                return;
            }
            Some((true, _)) => {}
        }
    }
    // Worn again, a file that could not be downloaded is asked for at
    // once.
    if let Some(id) = &id {
        wardrobe.missed.remove(id);
    }
    let mut looks = wardrobe.looks.clone();
    looks.wear(target, &name, category, id);
    if let Err(e) = looks.save(&home.0.root) {
        wardrobe.said =
            Some((format!("It could not be kept ({e:#})."), true));
        return;
    }
    wardrobe.looks = looks;
    wardrobe.said = None;
}

fn buy(
    asked: On<BuyAsset>,
    tokio: Res<Tokio>,
    market: Res<Market>,
    mut rounds: ResMut<Rounds<Dealt>>,
    mut wardrobe: ResMut<Wardrobe>,
) {
    let id = asked.event().0.clone();
    // A purchase asked while another waits on Stripe takes its place.
    // The one left is still the chief's if it is paid after all, once
    // the stall is asked again.
    if wardrobe.dealing()
        && wardrobe
            .deal
            .as_ref()
            .is_some_and(|(dealt, _)| *dealt == id)
    {
        return;
    }
    if !market.signed() {
        wardrobe.deal = Some((
            id,
            Deal::Failed("Sign in with GitHub to buy.".into()),
        ));
        return;
    }
    if let Some(task) = wardrobe.paying_task.take() {
        task.abort();
    }
    wardrobe.deal = Some((id.clone(), Deal::Buying));
    let client = market.client();
    let tell = rounds.begin();
    let task = tokio.0.spawn(async move {
        tell((id.clone(), pay(&client, &id, &tell).await));
    });
    wardrobe.paying_task = Some(task.abort_handle());
}

/// Buys the asset `id`, telling of the page to pay on, and waits for
/// the payment. What came of it.
async fn pay(
    client: &Client,
    id: &str,
    tell: &impl Fn(Dealt),
) -> Deal {
    let bought = match client.buy_asset(id).await {
        Ok(bought) => bought,
        Err(e) if refused(&e) == Some("conflict") => {
            return Deal::Done("You own it already.".into());
        }
        Err(e) => return Deal::Failed(format!("{e:#}")),
    };
    if bought.owned {
        return Deal::Done("It is yours.".into());
    }
    let (Some(purchase), Some(url)) =
        (bought.purchase_id, bought.checkout_url)
    else {
        return Deal::Failed(
            "The bazaar gave no page to pay on.".into(),
        );
    };
    let _ = browse(&url);
    tell((id.to_string(), Deal::Paying(url)));
    let until = tokio::time::Instant::now() + PAYMENT_PATIENCE;
    while tokio::time::Instant::now() < until {
        tokio::time::sleep(PAYMENT_POLL).await;
        match client.asset_paying(&purchase).await {
            Err(e) if signed_out(&e) => {
                return Deal::Failed("Signed out.".into());
            }
            Ok(Paying::Paid) => {
                return Deal::Done("It is yours.".into());
            }
            Ok(Paying::Expired) => {
                return Deal::Failed(
                    "The payment was not made. Nothing was charged."
                        .into(),
                );
            }
            // Pending, or a blip of the network, is waited out.
            _ => {}
        }
    }
    Deal::Failed("The payment was not finished in time.".into())
}

fn dealt(
    mut rounds: ResMut<Rounds<Dealt>>,
    mut wardrobe: ResMut<Wardrobe>,
    mut commands: Commands,
) {
    while let Some((id, deal)) = rounds.next() {
        if matches!(deal, Deal::Done(_)) {
            commands.trigger(AskWardrobe);
        }
        wardrobe.deal = Some((id, deal));
    }
}

#[cfg(test)]
mod tests {
    use hamlet_sandbox::Unsandboxed;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::runtime::Runtime;

    use super::*;
    use crate::testing::{self, place, until};

    fn asset(
        id: &str,
        category: &str,
        owned: bool,
    ) -> serde_json::Value {
        json!({"id": id, "target": "agent", "category": category,
            "title": id, "summary": "s", "priceCents": 99,
            "currency": "usd", "version": "1.0.0", "frames": 24,
            "previewUrl": format!("/v1/assets/{id}/preview/{{frame}}"),
            "owned": owned, "updatedAt": "2026-10-01T00:00:00Z"})
    }

    /// A stand-in for hamlet-server: two skins on the shelves, the
    /// chief owning the first.
    async fn serve(listener: TcpListener) {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut asked = Vec::new();
                let mut buf = [0u8; 4096];
                while !asked.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => asked.extend_from_slice(&buf[..n]),
                    }
                }
                let head =
                    String::from_utf8_lossy(&asked).into_owned();
                let line = head
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string();
                let body = if line.starts_with("GET /v1/me/assets") {
                    json!({"items": [asset("sakura", "skin", true)]})
                        .to_string()
                } else if line
                    .starts_with("GET /v1/assets/sakura/file")
                {
                    r##"{"format":1,"colours":{"paint":"#ffc0cb"}}"##
                        .to_string()
                } else if line.starts_with("GET /v1/assets") {
                    json!({"items": [asset("sakura", "skin", true), asset("ember", "skin", false)]}).to_string()
                } else {
                    String::new()
                };
                let answer = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: \
                     application/json\r\ncontent-length: \
                     {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(answer.as_bytes()).await;
            });
        }
    }

    #[test]
    fn a_file_that_failed_is_asked_for_again_after_a_while() {
        let skin = serde_json::from_value::<Asset>(asset(
            "sakura", "skin", true,
        ))
        .unwrap();
        let mut wardrobe = Wardrobe::default();
        wardrobe.owned.insert("sakura".into(), skin);
        wardrobe.looks.wear(
            Target::Agent,
            "juniper",
            Category::Skin,
            Some("sakura".into()),
        );
        let then = std::time::Instant::now();
        assert_eq!(wanted(&wardrobe, then).len(), 1);
        wardrobe.missed.insert("sakura".into(), then);
        wardrobe
            .files
            .insert("sakura".into(), File::Failed("down".into()));
        assert!(wanted(&wardrobe, then).is_empty());
        assert_eq!(wanted(&wardrobe, then + RETRY).len(), 1);
        wardrobe
            .files
            .insert("sakura".into(), File::Skin(Skin::default()));
        assert!(wanted(&wardrobe, then + RETRY).is_empty());
    }

    #[test]
    fn an_owned_skin_worn_is_downloaded_and_one_not_owned_is_refused()
    {
        let rt = Runtime::new().unwrap();
        let listener =
            rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let at = listener.local_addr().unwrap();
        rt.spawn(serve(listener));
        let home = place("wardrobe-wear");
        let mut app = testing::app(&home, &rt, &Unsandboxed, vec![]);
        app.update();
        app.world_mut()
            .resource_mut::<Market>()
            .sign_for_test(format!("http://{at}"), "hms_test");
        app.world_mut().trigger(AskWardrobe);
        until(&mut app, "the shelves to be read", |app| {
            app.world().resource::<Wardrobe>().owns("sakura")
        });
        let wear = |app: &mut App, id: &str| {
            app.world_mut().trigger(Wear {
                target: Target::Agent,
                name: "juniper".into(),
                category: Category::Skin,
                id: Some(id.into()),
            });
        };
        wear(&mut app, "ember");
        let wardrobe = app.world().resource::<Wardrobe>();
        assert!(
            wardrobe.said.as_ref().is_some_and(|(_, wrong)| *wrong)
        );
        assert!(
            wardrobe.looks.of(Target::Agent, "juniper").is_none()
        );

        wear(&mut app, "sakura");
        until(&mut app, "the skin to come", |app| {
            app.world()
                .resource::<Wardrobe>()
                .worn(Target::Agent, "juniper", Category::Skin)
                .is_some()
        });
        let wardrobe = app.world().resource::<Wardrobe>();
        let (_, File::Skin(skin)) = wardrobe
            .worn(Target::Agent, "juniper", Category::Skin)
            .unwrap()
        else {
            panic!("a skin");
        };
        assert_eq!(skin.colour("paint").unwrap()[0], 1.0);
        // Kept for the next run, the file itself nowhere on disk.
        assert_eq!(Looks::load(&home.root), wardrobe.looks);
        let on_disk = std::fs::read_dir(&home.root)
            .unwrap()
            .flatten()
            .filter(|entry| {
                std::fs::read(entry.path()).is_ok_and(|bytes| {
                    bytes.windows(7).any(|w| w == b"#ffc0cb")
                })
            })
            .count();
        assert_eq!(on_disk, 0);
    }
}
