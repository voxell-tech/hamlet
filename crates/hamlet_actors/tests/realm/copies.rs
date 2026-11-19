use hamlet_messages::realm::documents::{
    EditCharter, EditSettings, Setting,
};

use super::chiefs::chief;
use super::{MAYOR, Realm, founded, made_until};

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_is_made_with_the_chiefs_charter_and_model() {
    // the chief writes a charter and picks the realm's model, then
    // founds a hamlet: the mayor's create input holds both.
    let realm = Realm::start().await;
    chief(
        &realm,
        EditCharter {
            text: Some("Be brief.".to_owned()),
        },
    )
    .await;
    chief(
        &realm,
        EditSettings {
            set: vec![Setting::Model {
                model: "faux::faux-1".to_owned(),
            }],
        },
    )
    .await;

    founded(&realm).await;
    let copies = made_until(&realm, MAYOR).await.copies;

    assert_eq!(
        copies.charter.map(|charter| charter.value).as_deref(),
        Some("Be brief.")
    );
    assert_eq!(
        copies.model.map(|model| model.value.model).as_deref(),
        Some("faux::faux-1")
    );
    assert!(copies.settings.is_some());
    realm.host.shutdown().await.unwrap();
}
