//! The world a client reads, reaches pending until applied included.

#[tokio::test]
#[ignore = "the scaffold: U10 fills it"]
async fn the_world_answers_while_a_transaction_runs() {
    // while the test holds a write lock on the realm's file through
    // `rows()`, ReadWorld answers at once, from the copy.
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
    // a workspace stand-in that holds its answer; end a hire:
    // ReadWorld shows its reach Pending, then Applied once the
    // stand-in answers.
    todo!()
}
