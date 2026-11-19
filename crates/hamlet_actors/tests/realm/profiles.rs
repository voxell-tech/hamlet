use hamlet_messages::profile::{
    BadPath, Path, ProfileInput, ReadAnswer, VersionName,
};
use hamlet_messages::realm::documents::{EditSettings, Setting};
use hamlet_messages::realm::profiles::{
    CreateProfile, CreateRefusal, Made, ProfileRefusal, ReadProfile,
    ReadProfileRefusal,
};
use hamlet_messages::realm::{
    AgentRefusal, Done, Hire, HireAnswer, HireRefusal, LeadRefusal,
    ProfileId,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;

use super::chiefs::chief;
use super::steps::hiring_and_how;
use super::{
    MAYOR, Realm, THREE, founded, hamlet_with, hire, hired, tell, to,
    tool, words,
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
async fn a_blank_hire_starts_from_the_realms_character() {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let set = EditSettings {
        set: vec![Setting::Character {
            text: CHARACTER.to_owned(),
        }],
    };
    assert_eq!(chief(&realm, set).await, Done::Done);
    founded(&realm).await;
    realm.live(MAYOR).await;
    assert_eq!(
        hire_as_mayor(&realm, "h1", hire(None)).await,
        hired(3)
    );
    realm.live(THREE).await;

    assert_eq!(made(&realm, 2), Some(character(CHARACTER)));
    assert_eq!(made(&realm, 3), Some(character(CHARACTER)));
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
    assert!(words(told).contains("its task failed"));
    let (hiring, how) = hiring_and_how(&realm, THREE).await;
    assert_eq!(hiring, 0);
    assert_eq!(how.as_deref(), Some(r#"{"type":"failed"}"#));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_of_a_blank_hires_profile_names_it_by_its_label() {
    let (realm, _) = hamlet_with(0).await;
    assert_eq!(
        hire_as_mayor(&realm, "h1", named(" Digger ")).await,
        hired(3)
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

async fn create(
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

    assert_eq!(hire_as_mayor(&realm, "h1", at_v1).await, hired(3));
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
    assert_eq!(realm.seen.reads.lock().unwrap().len(), 3);
    realm.host.shutdown().await.unwrap();
}
