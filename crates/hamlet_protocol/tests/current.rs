//! Currency of the checked-in protocol types.

#[test]
fn generated_types_match_the_schema() {
    let schema = serde_json::from_str(include_str!(
        "../../../server/protocol.schema.json"
    ))
    .unwrap();
    let expected = hamlet_protocol_gen::generate(&schema).unwrap();
    assert_eq!(
        include_str!("../src/v0.rs"),
        expected,
        "Protocol types are stale. Run cargo run -p \
         hamlet_protocol_gen"
    );
}
