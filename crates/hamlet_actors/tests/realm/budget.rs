#[tokio::test]
#[ignore = "the scaffold: U7 fills it"]
async fn a_charge_repeated_with_one_id_counts_once() {
    // charge one call id twice: one row; the spend shows it once.
    todo!()
}

#[tokio::test]
#[ignore = "the scaffold: U7 fills it"]
async fn spending_past_the_budget_is_refused() {
    // set a small cost and time with Asked<SetLimits>. Charge past
    // the cost: OverCost; past the model time: OverTime; ResetBudget:
    // allowed again.
    todo!()
}

#[tokio::test]
#[ignore = "the scaffold: U7 fills it"]
async fn an_ended_agent_may_not_spend() {
    // end a hire; MaySpend as it: Left.
    todo!()
}
