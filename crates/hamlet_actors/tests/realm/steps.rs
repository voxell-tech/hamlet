//! A hire's steps, from its rows to `live`.

#[tokio::test]
#[ignore = "the scaffold: U3 fills it"]
async fn a_hire_goes_live_through_its_steps() {
    // found; wait live(2); the stand-ins saw the profile made, one
    // Branch `hires/2`, `agent/2` made with its place and standing
    // task, a Fill of `hires/2`, and a SetReach for agent 2.
    todo!()
}

#[tokio::test]
#[ignore = "the scaffold: U3 fills it"]
async fn a_hiring_row_is_finished_at_the_realms_start() {
    // a profile that fails `Branch` once; found; restart; live(2).
    todo!()
}

#[tokio::test]
#[ignore = "the scaffold: U3 fills it"]
async fn every_step_twice_leaves_one_hire() {
    // fail each step's callee once in turn; each run ends with one
    // agent 2, one branch, one SetReach kept.
    todo!()
}
