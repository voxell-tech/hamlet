//! What the chief does with a version of their listing: a draft's
//! details and cover saved, published unverified or paid to be
//! verified, then asked after until it is checked; a draft thrown
//! away; and the fees to verify, a version's files and the tags in
//! use asked for.

use std::collections::BTreeMap;

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Res;
use bevy::ecs::world::World;

use crate::bazaar::errand::Fetched;
use crate::bazaar::market::account::{Asking, browse};
use crate::bazaar::market::client::{Refusal, done, read};
use crate::bazaar::market::deal::PAYMENT_PATIENCE;
use crate::bazaar::market::lot::Paying;
use crate::bazaar::market::versions::{
    Details, Fees, File, FilesOf, Payment, TagCount, Verifying,
};
use crate::bazaar::market::{Market, account, send};
use crate::bazaar::shelf::now;

/// How often a version paid for is asked after until it is checked,
/// in milliseconds.
const EVERY: u64 = 5_000;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Drafts>().add_systems(
        Update,
        watch.run_if(|drafts: Res<Drafts>| drafts.paying.is_some()),
    );
}

/// The chief's work on the versions of their listings.
#[derive(Resource, Clone, Debug, PartialEq, Default)]
pub(crate) struct Drafts {
    pub fees: Option<Fees>,
    /// The files of versions, by listing and version.
    pub files: BTreeMap<(String, String), Files>,
    /// The tags in use that hold the words last asked about, and
    /// those words.
    pub tags: (String, Vec<TagCount>),
    /// What is being done, if anything.
    pub job: Option<Job>,
    /// What came of the last thing done, and whether it went wrong.
    pub said: Option<(String, bool)>,
    /// A version paid for on Stripe's page, asked after until it
    /// leaves its draft.
    pub paying: Option<Verification>,
}

/// The files of a version as they came.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Files {
    Fetching,
    Got(Vec<File>),
    Failed(String),
}

/// A version of a listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Which {
    pub id: String,
    pub version: String,
}

/// What is done after a draft's details are saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Then {
    Stay,
    Publish,
    Verify,
}

/// A draft's details saved, with its new cover, then what follows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Job {
    pub which: Which,
    pub details: Option<Details>,
    /// A picture for its cover and its media type.
    pub cover: Option<(Vec<u8>, String)>,
    pub then: Then,
    pub step: Step,
}

/// A step of a [`Job`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Saving,
    Covering,
    Publishing,
    Verifying,
    Discarding,
}

/// A version's verification paid for on Stripe's page, and the
/// asking after its payment.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Verification {
    pub which: Which,
    pub payment: String,
    pub asking: Asking,
}

/// A call of the server about a version.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Call {
    Fees,
    /// The tags in use that hold these words.
    Tags(String),
    Files(String, String),
    /// A step of the job going on.
    Job(Step),
    /// The payment of a verification asked after.
    Payment,
}

/// Asks for the fees to verify, once.
pub(crate) fn ask_fees(world: &mut World) {
    if world.resource::<Drafts>().fees.is_some() {
        return;
    }
    let request = world.resource::<Market>().server.sell_config();
    send(world, super::Call::Drafts(Call::Fees), request);
}

/// Asks for the tags in use that hold `words`, unless they were last.
pub(crate) fn ask_tags(world: &mut World, words: &str) {
    let words = words.trim().to_lowercase();
    if words.is_empty() || world.resource::<Drafts>().tags.0 == words
    {
        return;
    }
    world.resource_mut::<Drafts>().tags.0.clone_from(&words);
    let request = world.resource::<Market>().server.tags(&words);
    send(world, super::Call::Drafts(Call::Tags(words)), request);
}

/// Asks for the files of `version` of the listing `id` against the
/// version `against`, unless they came.
pub(crate) fn ask_files(
    world: &mut World,
    id: &str,
    version: &str,
    against: Option<&str>,
) {
    let key = (id.to_string(), version.to_string());
    let mut drafts = world.resource_mut::<Drafts>();
    if matches!(
        drafts.files.get(&key),
        Some(Files::Fetching | Files::Got(_))
    ) {
        return;
    }
    drafts.files.insert(key, Files::Fetching);
    let request = world
        .resource::<Market>()
        .server
        .files(id, version, against);
    let call = Call::Files(id.to_string(), version.to_string());
    send(world, super::Call::Drafts(call), request);
}

/// Saves the details of a draft, and its new cover, then does what
/// `then` says.
pub(crate) fn save(
    world: &mut World,
    which: Which,
    details: Details,
    cover: Option<(Vec<u8>, String)>,
    then: Then,
) {
    start(
        world,
        Job {
            which,
            details: Some(details),
            cover,
            then,
            step: Step::Saving,
        },
    );
}

/// Publishes a rejected version unverified, or pays for an unverified
/// one to be verified.
pub(crate) fn go_on(world: &mut World, which: Which, then: Then) {
    let step = match then {
        Then::Verify => Step::Verifying,
        _ => Step::Publishing,
    };
    start(
        world,
        Job {
            which,
            details: None,
            cover: None,
            then,
            step,
        },
    );
}

/// Throws a draft away.
pub(crate) fn discard(world: &mut World, which: Which) {
    start(
        world,
        Job {
            which,
            details: None,
            cover: None,
            then: Then::Stay,
            step: Step::Discarding,
        },
    );
}

fn start(world: &mut World, job: Job) {
    if world.resource::<Drafts>().job.is_some() {
        return;
    }
    let step = job.step;
    let mut drafts = world.resource_mut::<Drafts>();
    drafts.said = None;
    drafts.job = Some(job);
    ask(world, step);
}

fn ask(world: &mut World, step: Step) {
    let Some(job) = world.resource::<Drafts>().job.clone() else {
        return;
    };
    if let Some(job) = &mut world.resource_mut::<Drafts>().job {
        job.step = step;
    }
    let server = &world.resource::<Market>().server;
    let (id, version) = (&job.which.id, &job.which.version);
    let request = match step {
        Step::Saving => {
            let details = job.details.clone().unwrap_or_default();
            server.change_version(id, version, &details)
        }
        Step::Covering => {
            let Some((bytes, kind)) = job.cover else {
                return;
            };
            server.version_cover(id, version, bytes, &kind)
        }
        Step::Publishing => server.publish(id, version),
        Step::Verifying => server.verify_version(id, version),
        Step::Discarding => server.discard(id, version),
    };
    send(world, super::Call::Drafts(Call::Job(step)), request);
}

/// Ends the job going on, saying `said`.
fn finish(world: &mut World, said: String, wrong: bool) {
    let mut drafts = world.resource_mut::<Drafts>();
    drafts.job = None;
    drafts.said = Some((said, wrong));
    account::ask_mine(world);
}

/// Goes on from `step`, which went through.
fn after(world: &mut World, step: Step) {
    let Some(job) = world.resource::<Drafts>().job.clone() else {
        return;
    };
    let version = &job.which.version;
    match (step, job.then) {
        (Step::Saving, _) if job.cover.is_some() => {
            ask(world, Step::Covering);
        }
        (Step::Saving | Step::Covering, Then::Stay) => {
            finish(world, "Draft saved.".into(), false);
        }
        (Step::Saving | Step::Covering, Then::Publish) => {
            ask(world, Step::Publishing);
        }
        (Step::Saving | Step::Covering, Then::Verify) => {
            ask(world, Step::Verifying);
        }
        (Step::Publishing, _) => finish(
            world,
            format!("v{version} is live, unverified."),
            false,
        ),
        (Step::Discarding, _) => {
            finish(world, "Draft discarded.".into(), false);
        }
        (Step::Verifying, _) => {}
    }
}

fn watch(world: &mut World) {
    let now = now();
    let Some(paying) = world.resource::<Drafts>().paying.clone()
    else {
        return;
    };
    if paying.asking.over(now) {
        world.resource_mut::<Drafts>().paying = None;
        return;
    }
    if !paying.asking.due(now) {
        return;
    }
    if let Some(paying) = &mut world.resource_mut::<Drafts>().paying {
        paying.asking.out = true;
    }
    let request =
        world.resource::<Market>().server.payment(&paying.payment);
    send(world, super::Call::Drafts(Call::Payment), request);
}

/// Takes in where the payment of a verification stands: once paid,
/// the version is checked, and the checks' own watch asks after it.
fn paid(world: &mut World, payment: Result<Payment, Refusal>) {
    let now = now();
    let status = match payment {
        Ok(payment) => payment.status,
        // Asked again in a while.
        Err(_) => Paying::Pending,
    };
    let mut drafts = world.resource_mut::<Drafts>();
    let said = match status {
        Paying::Pending => {
            if let Some(paying) = &mut drafts.paying {
                paying.asking.heard(now);
            }
            return;
        }
        Paying::Paid => ("Paid. Our agents are checking it.", false),
        Paying::Expired | Paying::Failed => (
            "The payment did not go through. Pay again to verify it.",
            true,
        ),
    };
    drafts.paying = None;
    drafts.said = Some((said.0.into(), said.1));
    account::ask_mine(world);
}

pub(super) fn answered(
    world: &mut World,
    call: Call,
    answer: Fetched,
) {
    match call {
        Call::Fees => {
            if let Ok(fees) = read::<Fees>(answer) {
                world.resource_mut::<Drafts>().fees = Some(fees);
            }
        }
        Call::Tags(words) => {
            let tags =
                read::<Vec<TagCount>>(answer).unwrap_or_default();
            let mut drafts = world.resource_mut::<Drafts>();
            if drafts.tags.0 == words {
                drafts.tags.1 = tags;
            }
        }
        Call::Files(id, version) => {
            let files = match read::<FilesOf>(answer) {
                Ok(files) => Files::Got(files.files),
                Err(why) => Files::Failed(why.words()),
            };
            world
                .resource_mut::<Drafts>()
                .files
                .insert((id, version), files);
        }
        Call::Payment => paid(world, read::<Payment>(answer)),
        Call::Job(Step::Verifying) => match read::<Verifying>(answer)
        {
            Ok(verifying) => verified(world, &verifying),
            Err(why) => failed(world, &why),
        },
        Call::Job(step) => match done(answer) {
            Ok(()) => after(world, step),
            Err(why) => failed(world, &why),
        },
    }
}

/// Opens Stripe's page to pay for the verification, and asks after
/// the version until it is paid.
fn verified(world: &mut World, verifying: &Verifying) {
    browse(world, &verifying.checkout_url);
    let which = world
        .resource::<Drafts>()
        .job
        .as_ref()
        .map(|job| job.which.clone());
    if let Some(which) = which {
        world.resource_mut::<Drafts>().paying = Some(Verification {
            which,
            payment: verifying.payment_id.clone(),
            asking: Asking::new(now(), EVERY, PAYMENT_PATIENCE),
        });
    }
    finish(
        world,
        "Pay on Stripe's page in your browser. The checks start \
         once it is paid."
            .into(),
        false,
    );
}

fn failed(world: &mut World, why: &Refusal) {
    let words = match why.code.as_str() {
        "version_locked" => "This version is locked. Make a new \
                             version to change it."
            .into(),
        "kind_unavailable" => "Plugins can't be sold yet.".into(),
        _ => why.words(),
    };
    finish(world, words, true);
}
