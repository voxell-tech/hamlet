// This file was changed for Hamlet. See NOTICE in this crate.
use hamlet_actor_wire::shared_types::endpoint::parse_endpoint_auth;

#[test]
fn extracts_namespace_and_token_from_url_auth() {
    let parsed = parse_endpoint_auth(
        "https://my-ns:sk_secret@api.example.com",
    )
    .unwrap();
    assert_eq!(parsed.endpoint, "https://api.example.com/");
    assert_eq!(parsed.namespace.as_deref(), Some("my-ns"));
    assert_eq!(parsed.token.as_deref(), Some("sk_secret"));
}

#[test]
fn decodes_percent_encoded_credentials() {
    let parsed = parse_endpoint_auth(
        "https://cloud-ns:sk_cloud%2Dtoken@api.example.com",
    )
    .unwrap();
    assert_eq!(parsed.endpoint, "https://api.example.com/");
    assert_eq!(parsed.namespace.as_deref(), Some("cloud-ns"));
    assert_eq!(parsed.token.as_deref(), Some("sk_cloud-token"));
}

#[test]
fn supports_namespace_without_token() {
    let parsed =
        parse_endpoint_auth("https://my-ns@api.example.com").unwrap();
    assert_eq!(parsed.endpoint, "https://api.example.com/");
    assert_eq!(parsed.namespace.as_deref(), Some("my-ns"));
    assert_eq!(parsed.token, None);
}

#[test]
fn preserves_path_when_stripping_auth() {
    let parsed =
        parse_endpoint_auth("https://my-ns:tok@api.example.com/base")
            .unwrap();
    assert_eq!(parsed.endpoint, "https://api.example.com/base");
    assert_eq!(parsed.namespace.as_deref(), Some("my-ns"));
    assert_eq!(parsed.token.as_deref(), Some("tok"));
}

#[test]
fn normalizes_endpoint_without_auth() {
    let parsed =
        parse_endpoint_auth("http://127.0.0.1:6420").unwrap();
    assert_eq!(parsed.endpoint, "http://127.0.0.1:6420/");
    assert_eq!(parsed.namespace, None);
    assert_eq!(parsed.token, None);
}

// A missing scheme makes the namespace parse as one, producing an
// opaque URL with no auth. It must pass through instead of failing on
// credential stripping, matching the silent WHATWG setter no-op in
// TypeScript.
#[test]
fn passes_through_opaque_scheme_url() {
    let parsed =
        parse_endpoint_auth("my-ns:tok@api.example.com").unwrap();
    assert_eq!(parsed.endpoint, "my-ns:tok@api.example.com");
    assert_eq!(parsed.namespace, None);
    assert_eq!(parsed.token, None);
}

#[test]
fn rejects_token_without_namespace() {
    let error =
        parse_endpoint_auth("https://:sk_secret@api.example.com")
            .unwrap_err();
    assert!(error.to_string().contains("token without a namespace"));
}

#[test]
fn rejects_non_url_endpoint() {
    let error = parse_endpoint_auth("not a url").unwrap_err();
    assert!(error.to_string().contains("invalid URL"));
}

#[test]
fn rejects_query_string() {
    let error = parse_endpoint_auth(
        "https://my-ns:tok@api.example.com?foo=bar",
    )
    .unwrap_err();
    assert!(error.to_string().contains("query string"));
}

#[test]
fn rejects_fragment() {
    let error = parse_endpoint_auth(
        "https://my-ns:tok@api.example.com#section",
    )
    .unwrap_err();
    assert!(error.to_string().contains("fragment"));
}
