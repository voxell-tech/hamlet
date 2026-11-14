//! `Hire`, `End` and `Seat`, with a lead's powers.

#[tokio::test]
#[ignore = "the scaffold: U4 fills it"]
async fn a_lead_hires_and_ends_only_under_itself() {
    // mayor 2 hires 3 and 4 under 3; 4 may not end 3 (`NotALead`);
    // 3 ends 4.
    todo!()
}

#[tokio::test]
#[ignore = "the scaffold: U4 fills it"]
async fn an_end_moves_the_children_and_drops_the_reach() {
    // end 3 with 4 under it: 4's lead is 2; the workspace stand-in
    // kept an `Ended` reach for 3.
    todo!()
}

#[tokio::test]
#[ignore = "the scaffold: U4 fills it"]
async fn a_repeated_hire_hires_once() {
    // the same Tool<Hire> call id twice: one agent, the same answer.
    todo!()
}
