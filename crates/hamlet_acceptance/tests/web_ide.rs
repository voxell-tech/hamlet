//! The web IDE of the Files panel: one server for each hamlet, on
//! the folders it works on and, read-only, those it worked on before,
//! started anew when they change.

use std::collections::BTreeMap;

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use url::Url;

/// The query of the web IDE's address for `hamlet`.
async fn opened(
    chief: &mut Client,
    hamlet: &str,
) -> BTreeMap<String, String> {
    let answer = chief
        .ask(|id| {
            ClientMessage::WebIde(WebIde {
                hamlet: hamlet.into(),
                id,
            })
        })
        .await;
    let ServerMessage::Reply(reply) = answer else {
        panic!("a web IDE got {answer:?}");
    };
    assert_eq!(reply.kind, ReplyKind::Address, "{}", reply.text);
    Url::parse(&reply.text)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

/// The roots an address's query lists, with whether each is past.
fn roots(query: &BTreeMap<String, String>) -> Vec<(String, bool)> {
    let roots: Vec<serde_json::Value> =
        serde_json::from_str(&query["roots"]).unwrap();
    roots
        .into_iter()
        .map(|root| {
            (
                root["path"].as_str().unwrap().to_owned(),
                root["past"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn the_web_ide_shows_every_folder_and_those_dropped_read_only()
{
    let home = Home::new();
    let (app, docs) =
        (home.side().join("app"), home.side().join("docs"));
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&docs).unwrap();
    let (app, docs) =
        (app.display().to_string(), docs.display().to_string());
    let program = env!("CARGO_BIN_EXE_web_ide_stub");
    let mut env = QUIET.to_vec();
    env.push(("HAMLET_WEB_IDE", program));
    let mut s = Session::start_in(home, Faux::new(), &env).await;
    let ada = s.found("shop", "ada", "run the shop").await;
    let shop = s.chief.hamlet_id("shop");

    // One folder opens as it is.
    s.chief.command(ada, &format!("/folder add {app}")).await;
    let one = opened(&mut s.chief, &shop).await;
    assert_eq!(one.get("folder"), Some(&app));
    assert!(!one.contains_key("workspace"));
    assert_eq!(
        opened(&mut s.chief, &shop).await,
        one,
        "kept running"
    );

    // A second makes a workspace, on a server started anew.
    s.chief.command(ada, &format!("/folder add {docs}")).await;
    let two = opened(&mut s.chief, &shop).await;
    assert!(!two.contains_key("folder"));
    assert_ne!(two["tkn"], one["tkn"]);
    assert_eq!(
        roots(&two),
        [(app.clone(), false), (docs.clone(), false)]
    );
    let file: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&two["workspace"]).unwrap(),
    )
    .unwrap();
    assert_eq!(file["folders"].as_array().unwrap().len(), 2);

    // One dropped shows after the rest, read-only.
    s.chief.command(ada, &format!("/folder drop {app}")).await;
    let dropped = opened(&mut s.chief, &shop).await;
    assert_eq!(roots(&dropped), [(docs, false), (app.clone(), true)]);
    let file: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&dropped["workspace"]).unwrap(),
    )
    .unwrap();
    assert_eq!(
        file["settings"]["files.readonlyInclude"],
        serde_json::json!({ format!("{app}/**"): true })
    );
}
