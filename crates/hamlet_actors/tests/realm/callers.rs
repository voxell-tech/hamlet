use hamlet_messages::address;
use hamlet_messages::realm::documents::ReadCopies;
use hamlet_messages::realm::models::Credential;
use hamlet_messages::realm::{Asked, End, Secrets};
use hamlet_protocol::client::RequestId;

use super::{
    MAYOR, THREE, charge, error_code, hamlet_with, needs, say,
    standing, tell, to, tool,
};

#[tokio::test(flavor = "multi_thread")]
async fn each_kind_of_call_refuses_a_caller_it_does_not_admit() {
    // a chief's request from a hire, a hire's tool from the chief, a
    // line from a hire, the workspace's command from a hire, the
    // secret list from the chief, and an agent's read of a
    // credential, read of its copies and charge from the chief or
    // the workspace are each refused as a wrong caller.
    let (realm, _) = hamlet_with(1).await;
    let task = standing(&realm, THREE);
    let end = Asked {
        id: RequestId::new(),
        act: End {
            hire: to(THREE),
            why: None,
        },
    };
    let credential = Credential {
        provider: "anthropic".to_owned(),
    };
    let charged = charge("c1", &task, 1, 1);

    let codes = [
        error_code(realm.tool(MAYOR, &end).await),
        error_code(
            realm.ask(&tool("t1", tell(to(THREE), "hi"))).await,
        ),
        error_code(
            realm
                .tool(MAYOR, &say(&RequestId::new(), THREE, "hi"))
                .await,
        ),
        error_code(realm.tool(MAYOR, &needs(THREE, "c1")).await),
        error_code(realm.ask(&Secrets).await),
        error_code(realm.ask(&credential).await),
        error_code(realm.ask(&ReadCopies::default()).await),
        error_code(
            realm.call_as(&address::Workspace, &charged).await,
        ),
    ];

    assert_eq!(codes, ["message.wrong_caller"; 8].map(str::to_owned));
    realm.host.shutdown().await.unwrap();
}
