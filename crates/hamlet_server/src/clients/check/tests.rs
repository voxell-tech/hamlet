use hamlet_protocol::client::{ClientMessage, RequestId, VERSION};

use super::*;

#[test]
fn numeric_spelling_does_not_change_the_message() {
    let protocol = Protocol::new().unwrap();
    assert!(matches!(
        protocol.parse(&format!(
            r#"{{"type":"hello","v":"{VERSION}","token":"test"}}"#
        )),
        Ok(ClientMessage::Hello(_))
    ));
    let id = "00000000-0000-4000-8000-000000000001";
    let text = format!(
        r#"{{"type":"edit","doc":"settings","id":"{id}","set":{{"reflectEvery":2e0}}}}"#
    );
    assert!(matches!(
        protocol.parse(&text),
        Ok(ClientMessage::Edit(_))
    ));
    let invalid = text.replace("2e0", "2.5");
    let fault = protocol.parse(&invalid).unwrap_err();
    assert_eq!(fault.to.as_deref(), Some(id));
    assert_eq!(
        fault.text,
        "This `edit` message does not fit the protocol: \
         `set.reflectEvery` must be integer."
    );
}

#[test]
fn malformed_fields_are_correlated_only_to_a_valid_id() {
    let protocol = Protocol::new().unwrap();
    let id = "00000000-0000-4000-8000-000000000001";
    for (fields, words) in [
        (
            r#""doc":"character","scholar":1,"text":7"#,
            "`text` must be string",
        ),
        (
            r#""doc":"limits","hamlet":"home","set":{"section":"budget","key":"cost"}"#,
            "`set` lacks `value`",
        ),
        (
            r#""doc":"limits","hamlet":"home","whole":12"#,
            "`whole` must be object",
        ),
        (
            r#""doc":"settings","set":{"model":null}"#,
            "`set.model` must be string",
        ),
        (r#""doc":"charter","text":7"#, "`text` must be string"),
    ] {
        let message =
            format!(r#"{{"type":"edit","id":"{id}",{fields}}}"#);
        let fault = protocol.parse(&message).unwrap_err();
        assert_eq!(fault.to.as_deref(), Some(id));
        assert_eq!(
            fault.text,
            format!(
                "This `edit` message does not fit the protocol: \
                 {words}."
            )
        );
        assert_eq!(
            protocol
                .parse(&message.replace(id, "not-an-id"))
                .unwrap_err()
                .to,
            None
        );
    }
}

#[test]
fn the_schema_takes_as_an_id_just_what_request_id_does() {
    let protocol = Protocol::new().unwrap();
    let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
    for text in [
        id.to_owned(),
        id.to_uppercase(),
        id.replace('-', ""),
        format!("{{{id}}}"),
        format!("urn:uuid:{id}"),
        id.replacen('-', "", 1) + "0",
        id.replace('e', "g"),
        format!("{}\u{e9}", &id[..34]),
        String::new(),
    ] {
        let message = serde_json::json!({"type": "stop", "id": text})
            .to_string();
        match protocol.parse(&message) {
            Ok(_) => assert!(text.parse::<RequestId>().is_ok()),
            Err(fault) => {
                assert!(text.parse::<RequestId>().is_err());
                assert_eq!(
                    fault.text,
                    "This `stop` message does not fit the protocol: \
                     `id` must be a UUID."
                );
            }
        }
    }
}
