use hamlet_messages::profile::{
    BadPath, By, Edit, Path, ProfileInput, ReadAnswer, RevertAnswer,
    Revision, TagAnswer, TagRefusal, VersionName,
};
use hamlet_messages::realm::documents::{EditSettings, Setting};
use hamlet_messages::realm::profiles::{
    CreateProfile, CreateRefusal, EditProfile, EditProfileRefusal,
    Made, ProfileRefusal, ReadProfile, ReadProfileRefusal,
    RevertProfile, TagProfile, TagProfileRefusal,
};
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{
    AgentRefusal, Asked, ChiefRequest, Done, Found, FoundAnswer,
    FoundRefusal, Gate, Hire, HireAnswer, HireRefusal, LeadRefusal,
    ProfileId,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{Choice, RequestId};

use super::chiefs::chief;
use super::reserved::{in_turn, notices, waits};
use super::stand_ins::Callee;
use super::steps::hiring_and_how;
use super::{
    MAYOR, Realm, THREE, answer, chief_hires, found, founded,
    hamlet_with, hire, hired, named_as, tell, to, told_of_failed,
    tool, until, words,
};

const CHARACTER: &str = "You dig wells and keep them clean.";

fn made(realm: &Realm, profile: u32) -> Option<ProfileInput> {
    realm
        .seen
        .profiles
        .lock()
        .unwrap()
        .iter()
        .find(|(made, _)| *made == ProfileId(profile))
        .map(|(_, input)| input.clone())
}

fn character(text: &str) -> ProfileInput {
    ProfileInput::Character {
        text: text.to_owned(),
    }
}

async fn hire_as_mayor(
    realm: &Realm,
    call: &str,
    hiring: Hire,
) -> HireAnswer<LeadRefusal<HireRefusal>> {
    realm.tool(MAYOR, &tool(call, hiring)).await.unwrap()
}

fn of(profile: u32) -> Hire {
    Hire {
        profile: Some(ProfileId(profile)),
        ..hire(None)
    }
}

fn named(name: &str) -> Hire {
    Hire {
        name: Some(name.to_owned()),
        ..hire(None)
    }
}

fn refused(why: HireRefusal) -> HireAnswer<LeadRefusal<HireRefusal>> {
    HireAnswer::Refused {
        why: LeadRefusal::Act(why),
    }
}

fn apart(label: &str) -> HireAnswer<LeadRefusal<HireRefusal>> {
    refused(HireRefusal::Apart {
        label: label.to_owned(),
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agents_blank_hire_starts_from_the_realms_character_and_a_chiefs_from_none()
 {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let set = EditSettings {
        set: vec![Setting::Character {
            text: CHARACTER.to_owned(),
        }],
    };
    assert_eq!(chief(&realm, set).await, Done::Done);
    let mill = founded(&realm).await;
    realm.live(MAYOR).await;
    assert_eq!(
        hire_as_mayor(&realm, "h1", hire(None)).await,
        hired(3)
    );
    assert_eq!(
        chief_hires(&realm, &mill, hire(None)).await,
        hired(4)
    );
    let quay = Found {
        purpose: Some("Mind the boats".to_owned()),
        ..found("Quay")
    };
    let FoundAnswer::Founded { mayor, .. } =
        realm.tool(Hand::NUMBER, &tool("f1", quay)).await.unwrap()
    else {
        panic!("the Hand founds Quay");
    };
    for agent in [THREE, AgentNumber(4), mayor] {
        realm.live(agent).await;
    }

    assert_eq!(made(&realm, 2), Some(character("")));
    assert_eq!(made(&realm, 3), Some(character(CHARACTER)));
    assert_eq!(made(&realm, 4), Some(character("")));
    assert_eq!(made(&realm, mayor.0), Some(character(CHARACTER)));
    let Some(ProfileInput::Character { text }) = made(&realm, 1)
    else {
        panic!("the Hand's profile is made from a character");
    };
    assert!(text.starts_with("You are the Hand"), "{text}");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_at_a_version_its_profile_lacks_ends_as_failed() {
    let (realm, _) = hamlet_with(0).await;
    let at_v9 = Hire {
        version: Some(VersionName("v9".to_owned())),
        ..hire(None)
    };
    assert_eq!(hire_as_mayor(&realm, "h1", at_v9).await, hired(3));

    let told = realm
        .tool(MAYOR, &tool("t1", tell(to(THREE), "dig the well")))
        .await
        .unwrap();

    let branched = realm.seen.branches.lock().unwrap().clone();
    assert!(
        branched.iter().any(|(profile, branch)| {
            *profile == ProfileId(3)
                && branch.from == Some(VersionName("v9".to_owned()))
        }),
        "{branched:?}"
    );
    assert_eq!(words(told), "no agent has the address `hire#3`");
    let (hiring, how) = hiring_and_how(&realm, THREE).await;
    assert_eq!(hiring, 0);
    assert_eq!(how.as_deref(), Some(r#"{"type":"failed"}"#));
    assert_eq!(
        told_of_failed(&realm, MAYOR, THREE).await,
        "`hire#3 (Hire 1)` has left: its task failed: `scholar#3` \
         has no version `v9`"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_of_a_blank_hires_profile_names_it_by_its_label() {
    let (realm, _) = hamlet_with(0).await;
    assert_eq!(
        hire_as_mayor(&realm, "h1", named(" Digger ")).await,
        HireAnswer::Hired {
            agent: named_as(3, "hire#3 (Digger)")
        }
    );
    assert_eq!(
        hire_as_mayor(&realm, "h2", hire(None)).await,
        hired(4)
    );

    assert_eq!(
        hire_as_mayor(&realm, "h3", of(2)).await,
        apart("scholar#2 (Mayor)")
    );
    assert_eq!(
        hire_as_mayor(&realm, "h4", of(3)).await,
        apart("scholar#3 (Digger)")
    );
    assert_eq!(
        hire_as_mayor(&realm, "h5", of(4)).await,
        apart("scholar#4 (Hire 2)")
    );
    for profile in [1, 9] {
        assert_eq!(
            hire_as_mayor(
                &realm,
                &format!("n{profile}"),
                of(profile)
            )
            .await,
            refused(HireRefusal::NoSuchProfile {
                profile: ProfileId(profile),
            })
        );
    }
    realm.host.shutdown().await.unwrap();
}

type CreateAnswer = Made<AgentRefusal<CreateRefusal>>;

pub(super) async fn create(
    realm: &Realm,
    by: AgentNumber,
    call: &str,
    name: &str,
    character: Option<&str>,
) -> CreateAnswer {
    let made = CreateProfile {
        name: name.to_owned(),
        character: character.map(str::to_owned),
    };
    realm.tool(by, &tool(call, made)).await.unwrap()
}

fn made_as(profile: u32, label: &str) -> CreateAnswer {
    Made::Made {
        profile: ProfileId(profile),
        label: label.to_owned(),
    }
}

async fn set_character(realm: &Realm) {
    let set = EditSettings {
        set: vec![Setting::Character {
            text: CHARACTER.to_owned(),
        }],
    };
    assert_eq!(chief(realm, set).await, Done::Done);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_made_profile_is_made_from_its_character_before_the_answer()
{
    let (realm, _) = hamlet_with(0).await;
    set_character(&realm).await;

    let by_hand = create(
        &realm,
        Hand::NUMBER,
        "c1",
        " Well digger ",
        Some("You dig."),
    )
    .await;
    assert_eq!(made(&realm, 3), Some(character("You dig.")));
    let by_mayor = create(&realm, MAYOR, "c2", "Mason", None).await;
    assert_eq!(made(&realm, 4), Some(character(CHARACTER)));
    let again = create(&realm, MAYOR, "c2", "Mason", None).await;

    assert_eq!(by_hand, made_as(3, "scholar#3 (Well digger)"));
    assert_eq!(by_mayor, made_as(4, "scholar#4 (Mason)"));
    assert_eq!(again, by_mayor);
    assert_eq!(realm.seen.profiles.lock().unwrap().len(), 4);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_is_not_made_with_a_bad_name_or_a_long_character() {
    let (realm, _) = hamlet_with(0).await;
    let long = "a".repeat(4001);

    let refused = |why| Made::Refused {
        why: AgentRefusal::Act(why),
    };
    assert_eq!(
        create(&realm, MAYOR, "c1", "a: b", None).await,
        refused(CreateRefusal::BadName {
            name: "a: b".to_owned(),
        })
    );
    assert_eq!(
        create(&realm, MAYOR, "c2", "Mason", Some(&long)).await,
        refused(CreateRefusal::LongCharacter)
    );
    assert_eq!(realm.seen.profiles.lock().unwrap().len(), 2);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_of_a_made_profile_at_a_version_starts_its_branch_there()
 {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    let at_v1 = Hire {
        version: Some(VersionName("v1".to_owned())),
        ..of(3)
    };

    assert_eq!(
        hire_as_mayor(&realm, "h1", at_v1).await,
        HireAnswer::Hired {
            agent: named_as(3, "hire#3 (Mason)")
        }
    );
    realm.live(THREE).await;

    let branched = realm.seen.branches.lock().unwrap().clone();
    assert!(
        branched.iter().any(|(profile, branch)| {
            *profile == ProfileId(3)
                && branch.name == "hires/3"
                && branch.from == Some(VersionName("v1".to_owned()))
        }),
        "{branched:?}"
    );
    assert_eq!(realm.seen.profiles.lock().unwrap().len(), 3);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_at_a_version_its_profile_lacks_is_refused_by_name() {
    let (realm, hamlet) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    let branched = || realm.seen.branches.lock().unwrap().len();
    let before = branched();
    let at = |version: &str| Hire {
        version: Some(VersionName(version.to_owned())),
        ..of(3)
    };
    let refusal =
        |answer: HireAnswer<LeadRefusal<HireRefusal>>| match answer {
            HireAnswer::Refused { why } => why.to_string(),
            HireAnswer::Hired { agent } => {
                format!("hired {agent}")
            }
        };

    for version in ["v9", "main"] {
        assert_eq!(
            refusal(
                hire_as_mayor(&realm, version, at(version)).await
            ),
            format!("`scholar#3 (Mason)` has no version `{version}`")
        );
    }
    let by_the_chief =
        match chief_hires(&realm, &hamlet, at("v9")).await {
            HireAnswer::Refused { why } => why.to_string(),
            HireAnswer::Hired { agent } => {
                format!("hired {agent}")
            }
        };
    assert_eq!(
        by_the_chief,
        "`scholar#3 (Mason)` has no version `v9`"
    );
    assert_eq!(branched(), before);
    assert_eq!(
        hire_as_mayor(&realm, "v1", at("v1")).await,
        HireAnswer::Hired {
            agent: named_as(3, "hire#3 (Mason)")
        }
    );
    realm.host.shutdown().await.unwrap();
}

/// The profile `Mason`, made by the Hand, as the mayor of a hamlet
/// founded at `version`.
fn quay_at(version: &str) -> Found {
    Found {
        mayor: Some(ProfileId(3)),
        version: Some(VersionName(version.to_owned())),
        purpose: Some("Mind the boats".to_owned()),
        ..found("Quay")
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_found_at_a_version_seats_its_mayor_at_it() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;

    let FoundAnswer::Founded { mayor, .. } = realm
        .tool(Hand::NUMBER, &tool("f1", quay_at("v1")))
        .await
        .unwrap()
    else {
        panic!("the Hand founds Quay");
    };
    realm.live(mayor).await;

    let branched = realm.seen.branches.lock().unwrap().clone();
    assert!(
        branched.iter().any(|(profile, branch)| {
            *profile == ProfileId(3)
                && branch.name == format!("hires/{}", mayor.0)
                && branch.from == Some(VersionName("v1".to_owned()))
        }),
        "{branched:?}"
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_found_at_a_version_its_profile_lacks_is_refused_by_name() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    let branched = || realm.seen.branches.lock().unwrap().len();
    let before = branched();
    let refusal = |answer: FoundAnswer<FoundRefusal>| match answer {
        FoundAnswer::Refused { why } => why.to_string(),
        FoundAnswer::Founded { hamlet, .. } => {
            format!("founded {}", hamlet.0)
        }
    };

    let by_the_hand = realm
        .tool(Hand::NUMBER, &tool("f1", quay_at("main")))
        .await
        .unwrap();
    let by_the_chief = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: quay_at("v9"),
        })
        .await
        .unwrap();

    assert_eq!(
        refusal(by_the_hand),
        "`scholar#3 (Mason)` has no version `main`"
    );
    assert_eq!(
        refusal(by_the_chief),
        "`scholar#3 (Mason)` has no version `v9`"
    );
    assert_eq!(branched(), before);
    realm.host.shutdown().await.unwrap();
}

fn at_v1() -> Hire {
    Hire {
        version: Some(VersionName("v1".to_owned())),
        ..of(3)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_hire_at_a_version_answers_as_it_first_did() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    let first = hire_as_mayor(&realm, "v1", at_v1()).await;
    assert_eq!(
        first,
        HireAnswer::Hired {
            agent: named_as(3, "hire#3 (Mason)")
        }
    );

    realm.seen.hold(Callee::Read);
    let again = hire_as_mayor(&realm, "v1", at_v1()).await;
    realm.seen.release(Callee::Read);

    assert_eq!(again, first);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_whose_profile_does_not_show_its_version_in_time_hears_so()
 {
    let (realm, hamlet) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    let heard = "`scholar#3 (Mason)` did not answer in time to show \
                 its version `v1`; try the hire again";

    realm.seen.hold(Callee::Read);
    let by_an_agent = hire_as_mayor(&realm, "v1", at_v1()).await;
    let by_the_chief = chief_hires(&realm, &hamlet, at_v1()).await;
    realm.seen.release(Callee::Read);

    assert_eq!(words(by_an_agent), heard);
    assert_eq!(words(by_the_chief), heard);
    assert_eq!(
        hire_as_mayor(&realm, "v1", at_v1()).await,
        HireAnswer::Hired {
            agent: named_as(3, "hire#3 (Mason)")
        }
    );
    realm.host.shutdown().await.unwrap();
}

type ReadProfileAnswer = ReadAnswer<AgentRefusal<ReadProfileRefusal>>;

async fn read(
    realm: &Realm,
    by: AgentNumber,
    profile: u32,
    path: Option<&str>,
) -> anyhow::Result<ReadProfileAnswer> {
    let asked = ReadProfile {
        profile: ProfileId(profile),
        at: None,
        path: path.map(str::to_owned),
    };
    realm.tool(by, &tool("r", asked)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hand_reads_a_made_profile_in_the_profiles_words() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    let refused = |why: ReadProfileRefusal| ReadAnswer::Refused {
        why: AgentRefusal::Act(why),
    };

    assert_eq!(
        read(&realm, Hand::NUMBER, 3, None).await.unwrap(),
        ReadAnswer::Listed {
            paths: vec![Path {
                path: "CHARACTER.md".to_owned(),
            }],
        }
    );
    assert_eq!(
        read(&realm, Hand::NUMBER, 3, Some("")).await.unwrap(),
        read(&realm, Hand::NUMBER, 3, None).await.unwrap(),
        "an empty path lists the files"
    );
    assert_eq!(
        read(&realm, Hand::NUMBER, 3, Some("CHARACTER.md"))
            .await
            .unwrap(),
        ReadAnswer::Text {
            text: "scholar#3's character".to_owned(),
            cut: false,
        }
    );
    assert_eq!(
        read(&realm, Hand::NUMBER, 3, Some("x.md")).await.unwrap(),
        refused(ReadProfileRefusal::Path(BadPath::NotAPath {
            path: "x.md".to_owned(),
        }))
    );
    assert_eq!(
        read(&realm, Hand::NUMBER, 2, None).await.unwrap(),
        refused(ReadProfileRefusal::Profile(ProfileRefusal::Apart {
            label: "scholar#2 (Mayor)".to_owned(),
        }))
    );
    assert_eq!(
        read(&realm, Hand::NUMBER, 9, None).await.unwrap(),
        refused(ReadProfileRefusal::Profile(
            ProfileRefusal::NoSuchProfile {
                profile: ProfileId(9),
            }
        ))
    );
    assert!(read(&realm, MAYOR, 3, None).await.is_err());
    assert_eq!(realm.seen.reads.lock().unwrap().len(), 5);
    realm.host.shutdown().await.unwrap();
}

fn edit(profile: u32, path: &str) -> EditProfile {
    EditProfile {
        profile: ProfileId(profile),
        path: path.to_owned(),
        text: "dig deep".to_owned(),
        message: None,
    }
}

fn tag(version: &str) -> TagProfile {
    TagProfile {
        profile: ProfileId(3),
        version: VersionName(version.to_owned()),
        at: None,
        note: String::new(),
    }
}

fn revert() -> RevertProfile {
    RevertProfile {
        profile: ProfileId(3),
        what: Revision::Version {
            version: VersionName("v1".to_owned()),
        },
        reason: None,
    }
}

fn edits(realm: &Realm) -> Vec<(ProfileId, Edit)> {
    realm.seen.edits.lock().unwrap().clone()
}

async fn notices_until(realm: &Realm, count: usize) -> Vec<String> {
    let seen = || {
        super::submits_to(realm, Hand::NUMBER)
            .iter()
            .filter(|submit| {
                matches!(
                    submit.input,
                    hamlet_messages::agent::Input::Notice { .. }
                )
            })
            .count()
    };
    until(&format!("the Hand is told {count} notices"), || {
        seen() >= count
    })
    .await;
    notices(realm, Hand::NUMBER).await
}

fn reserved<R>(wait: u64) -> AgentRefusal<R> {
    AgentRefusal::Gate(Gate::Reserved { wait: WaitId(wait) })
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hands_change_to_a_profile_runs_once_a_chief_allows_it() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;

    let edited = realm
        .tool(Hand::NUMBER, &in_turn("e1", edit(3, "notes.md"), None))
        .await
        .unwrap();
    let tagged = realm
        .tool(Hand::NUMBER, &in_turn("t1", tag("v2"), None))
        .await
        .unwrap();
    let reverted = realm
        .tool(Hand::NUMBER, &in_turn("r1", revert(), None))
        .await
        .unwrap();
    let asked = edits(&realm);
    let subjects = waits(&realm)
        .await
        .into_iter()
        .map(|(_, kind, subject, _)| (kind, subject))
        .collect::<Vec<_>>();
    let allow_edit = answer("#1", Choice::Allow, None);
    for allow in [
        allow_edit.clone(),
        answer("#2", Choice::Allow, None),
        answer("#3", Choice::Allow, None),
    ] {
        assert_eq!(realm.ask(&allow).await.unwrap(), Done::Done);
    }
    let told = notices_until(&realm, 3).await;
    assert_eq!(realm.ask(&allow_edit).await.unwrap(), Done::Done);
    until("the repeat reaches the profile", || {
        edits(&realm).len() == 2
    })
    .await;

    assert_eq!(edited, Done::Refused { why: reserved(1) });
    assert_eq!(tagged, TagAnswer::Refused { why: reserved(2) });
    assert_eq!(reverted, RevertAnswer::Refused { why: reserved(3) });
    assert!(
        asked.is_empty(),
        "nothing runs before a chief allows it"
    );
    assert_eq!(
        subjects,
        [
            (
                "reserved".to_owned(),
                r#"edit_scholar {"content":"dig deep","path":"notes.md","scholar":"scholar#3"}"#
                    .to_owned()
            ),
            (
                "reserved".to_owned(),
                r#"tag {"scholar":"scholar#3","tag":"v2"}"#.to_owned()
            ),
            (
                "reserved".to_owned(),
                r#"revert {"commit":"v1","scholar":"scholar#3"}"#
                    .to_owned()
            ),
        ]
    );
    assert_eq!(
        told,
        [
            "Wrote notes.md of scholar#3.",
            "Tagged scholar#3's main as v2.",
            "Reverted the commit on scholar#3's main; these files \
             changed again since and kept their later lines: \
             notes.md",
        ]
    );
    let [(profile, first), (_, again)] = &edits(&realm)[..] else {
        panic!("the edit and its repeat");
    };
    assert_eq!(*profile, ProfileId(3));
    assert_eq!(
        first.by,
        By::Agent {
            agent: Hand::NUMBER
        }
    );
    assert_eq!(first.id, again.id, "a repeat asks with the same id");
    let tags = realm.seen.tags.lock().unwrap().clone();
    assert_ne!(tags[0].1.id, first.id, "each call has its own id");
    realm.host.shutdown().await.unwrap();
}

/// The profile fails the first edit as an actor not ready; the realm
/// makes the notice again a second later, and with it the edit.
#[tokio::test(flavor = "multi_thread")]
async fn an_allowed_change_that_fails_is_made_again_until_done() {
    let edits =
        allowed_once_failing(Callee::Edit, AgentNumber(3)).await;

    let [(_, failed), (_, ran)] = &edits[..] else {
        panic!("the failed edit and the one that ran");
    };
    assert_eq!(ran.id, failed.id, "made again with the same id");
}

/// The Hand's agent fails the first notice as an actor not ready; the
/// realm makes the notice again a second later, and with it the edit,
/// which the profile answers the second time as a repeat.
#[tokio::test(flavor = "multi_thread")]
async fn an_allowed_changes_notice_that_fails_is_made_again_until_taken()
 {
    let edits =
        allowed_once_failing(Callee::Submit, Hand::NUMBER).await;

    let [(_, ran), (_, repeat)] = &edits[..] else {
        panic!("the edit that ran and its repeat");
    };
    assert_eq!(repeat.id, ran.id, "made again with the same id");
}

/// The edits the profile was asked for when the Hand's allowed edit
/// ran with `callee` failing once for `agent`, after its notice was
/// taken.
async fn allowed_once_failing(
    callee: Callee,
    agent: AgentNumber,
) -> Vec<(ProfileId, Edit)> {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    realm
        .tool(Hand::NUMBER, &in_turn("e1", edit(3, "notes.md"), None))
        .await
        .unwrap();
    realm.seen.fail_once(callee, agent);

    let allow = answer("#1", Choice::Allow, None);
    assert_eq!(realm.ask(&allow).await.unwrap(), Done::Done);

    assert_eq!(
        notices_until(&realm, 1).await,
        ["Wrote notes.md of scholar#3."]
    );
    let edits = edits(&realm);
    realm.host.shutdown().await.unwrap();
    edits
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allowed_change_that_is_refused_is_told_in_its_words() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;

    for (call, edited) in
        [("e1", edit(2, "notes.md")), ("e2", edit(3, "../out"))]
    {
        realm
            .tool(Hand::NUMBER, &in_turn(call, edited, None))
            .await
            .unwrap();
    }
    for wait in ["#1", "#2"] {
        let allow = answer(wait, Choice::Allow, None);
        assert_eq!(realm.ask(&allow).await.unwrap(), Done::Done);
    }

    assert_eq!(
        notices_until(&realm, 2).await,
        [
            "`scholar#2 (Mayor)` is a blank hire's own, not a \
             scholar of the city",
            "there is no file `../out` here",
        ]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_hand_changes_a_profile() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;

    let by_mayor = realm
        .tool(MAYOR, &in_turn("e1", edit(3, "notes.md"), None))
        .await;

    assert!(by_mayor.is_err(), "{by_mayor:?}");
    assert!(waits(&realm).await.is_empty());
    realm.host.shutdown().await.unwrap();
}

pub(super) async fn asked<T: ChiefRequest>(
    realm: &Realm,
    id: &RequestId,
    act: T,
) -> T::Answer {
    realm
        .ask(&Asked {
            id: id.clone(),
            act,
        })
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chiefs_change_to_a_profile_is_answered_from_the_profile() {
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    let id = RequestId::new();

    let edited = asked(&realm, &id, edit(3, "notes.md")).await;
    let tagged = asked(&realm, &RequestId::new(), tag("v2")).await;
    let taken = asked(&realm, &RequestId::new(), tag("v1")).await;
    let reverted = asked(&realm, &RequestId::new(), revert()).await;
    let apart =
        asked(&realm, &RequestId::new(), edit(2, "notes.md")).await;
    let missing =
        asked(&realm, &RequestId::new(), edit(9, "notes.md")).await;

    assert_eq!(edited, Done::Done);
    assert_eq!(
        edits(&realm),
        [(
            ProfileId(3),
            Edit {
                id,
                path: "notes.md".to_owned(),
                text: "dig deep".to_owned(),
                message: None,
                by: By::Chief { chief: 1 },
            }
        )]
    );
    let TagAnswer::Tagged { version, .. } = tagged else {
        panic!("tagged: {tagged:?}");
    };
    assert_eq!(version, VersionName("v2".to_owned()));
    assert_eq!(
        taken,
        TagAnswer::Refused {
            why: TagProfileRefusal::Tag(TagRefusal::Taken {
                version: VersionName("v1".to_owned()),
            }),
        }
    );
    let RevertAnswer::Reverted { clashes, .. } = reverted else {
        panic!("reverted: {reverted:?}");
    };
    assert_eq!(
        clashes,
        [Path {
            path: "notes.md".to_owned(),
        }]
    );
    assert_eq!(
        apart,
        Done::Refused {
            why: EditProfileRefusal::Profile(ProfileRefusal::Apart {
                label: "scholar#2 (Mayor)".to_owned(),
            }),
        }
    );
    assert_eq!(
        missing,
        Done::Refused {
            why: EditProfileRefusal::Profile(
                ProfileRefusal::NoSuchProfile {
                    profile: ProfileId(9),
                }
            ),
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_at_another_hires_branch_starts_its_branch_there() {
    // As today's hire at a version, which takes a branch's name
    // (`hamlet_git` `resolve`).
    let (realm, _) = hamlet_with(0).await;
    create(&realm, Hand::NUMBER, "c1", "Mason", None).await;
    let at = |version: &str| Hire {
        version: Some(VersionName(version.to_owned())),
        ..of(3)
    };
    let branched = |name: &str| {
        realm.seen.branches.lock().unwrap().iter().find_map(
            |(_, branch)| {
                (branch.name == name).then(|| branch.from.clone())
            },
        )
    };
    hire_as_mayor(&realm, "h1", at("v1")).await;
    until("hire 3's branch is made", || {
        branched("hires/3").is_some()
    })
    .await;

    let at_its_branch =
        hire_as_mayor(&realm, "h2", at("hires/3")).await;
    until("hire 4's branch is made", || {
        branched("hires/4").is_some()
    })
    .await;

    assert_eq!(
        at_its_branch,
        HireAnswer::Hired {
            agent: named_as(4, "hire#4 (Mason)")
        }
    );
    assert_eq!(
        branched("hires/4"),
        Some(Some(VersionName("hires/3".to_owned())))
    );
    realm.host.shutdown().await.unwrap();
}
