use hamlet_messages::profile::{ProfileInput, VersionName};
use hamlet_messages::realm::documents::{EditSettings, Setting};
use hamlet_messages::realm::{
    Done, Hire, HireAnswer, HireRefusal, LeadRefusal, ProfileId,
};
use hamlet_messages::sender::Hand;

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
