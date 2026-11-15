#[tokio::test]
#[ignore = "the scaffold: U10 fills it"]
async fn the_world_answers_while_a_transaction_runs() {
    // The host holds the realm's file locked while the realm runs,
    // so no test can take its write lock. The root rules how U10
    // shows that ReadWorld answers from the copy while a transaction
    // runs.
    todo!()
}

#[tokio::test]
#[ignore = "the scaffold: U10 fills it"]
async fn the_revision_survives_a_restart() {
    // found; read the revision; restart; it is the same.
    todo!()
}

#[tokio::test]
#[ignore = "the scaffold: U10 fills it"]
async fn a_revoke_shows_pending_until_the_workspace_answers() {
    // hold SetReach; end a hire: ReadWorld shows its reach Pending;
    // release SetReach: Applied.
    todo!()
}
