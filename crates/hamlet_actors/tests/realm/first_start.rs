use hamlet_messages::realm::budget::Payer;
use rusqlite::OptionalExtension;

use super::Realm;

#[tokio::test(flavor = "multi_thread")]
async fn a_new_realm_holds_the_hand_as_agent_1() {
    // start; read the rows: agent 1 is the Hand of profile 1,
    // `hiring` or `live` as far as its drive went; its hire row is
    // closed once `live`; its standing task is paid by the realm.
    let realm = Realm::start().await;

    let (agent, profile, row, payer) = realm
        .stopped_rows(|db| {
            let agent = db
                .query_row(
                    "SELECT kind, status FROM agent WHERE id = 1",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                        ))
                    },
                )
                .optional()?;
            let profile = db
                .query_row(
                    "SELECT kind FROM profile WHERE id = 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            let row = db
                .query_row(
                    "SELECT done, closed FROM step WHERE kind = \
                     'hire' AND n = 1",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, i64>(1)?,
                        ))
                    },
                )
                .optional()?;
            let payer = db
                .query_row(
                    "SELECT payer FROM task WHERE agent = 1 AND \
                     origin = 'standing'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            Ok((agent, profile, row, payer))
        })
        .await;

    assert!(
        matches!(
            agent.as_ref().map(|(kind, status)| (&**kind, &**status)),
            Some(("hand", "hiring" | "live"))
        ),
        "{agent:?}"
    );
    assert_eq!(profile.as_deref(), Some("hand"));
    let (done, closed) = row.unwrap();
    assert_eq!(
        closed == 1,
        done.as_deref() == Some("live"),
        "{done:?}"
    );
    let payer =
        serde_json::from_str::<Payer>(&payer.unwrap()).unwrap();
    assert_eq!(payer, Payer::Realm);
    realm.host.shutdown().await.unwrap();
}
