//! Currency of the checked-in harness link types.

#[test]
fn generated_link_types_match_the_schema() {
    let schema = serde_json::from_str(include_str!(
        "../../../harnesses/pi/link.schema.json"
    ))
    .unwrap();
    let expected = hamlet_protocol_gen::generate(&schema).unwrap();
    assert_eq!(
        include_str!("../src/harness/link.rs"),
        expected,
        "Link types are stale. Run npm run schema --prefix \
         harnesses/pi, then cargo run -p hamlet_protocol_gen"
    );
}
