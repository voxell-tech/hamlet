// This file was changed for Hamlet. See NOTICE in this crate.
mod moved_tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use hamlet_actor_error::ActorError;

    use crate::actor::action::ActionDispatchError;
    use crate::actor::messages::Request;
    use crate::error::ActorLifecycle as ActorLifecycleError;
    use crate::host::config::HttpRequestBodyStream;
    use crate::host::dispatch::{
        with_action_dispatch_timeout, with_framework_action_timeout,
        workflow_dispatch_result,
    };
    use crate::host::http::{
        HttpResponseEncoding, authorization_bearer_token,
        authorization_bearer_token_map, is_actor_request_path,
        normalize_actor_request_path, request_encoding,
    };

    #[test]
    fn workflow_dispatch_result_marks_handled_workflow_as_enabled() {
        assert_eq!(
            workflow_dispatch_result(Ok(Some(vec![1, 2, 3])))
                .expect("workflow dispatch should succeed"),
            (true, Some(vec![1, 2, 3])),
        );
        assert_eq!(
            workflow_dispatch_result(Ok(None))
                .expect("workflow dispatch should succeed"),
            (true, None),
        );
    }

    #[test]
    fn workflow_dispatch_result_treats_dropped_reply_as_disabled() {
        assert_eq!(
            workflow_dispatch_result(Err(
                ActorLifecycleError::DroppedReply.build()
            ))
            .expect("dropped reply should map to workflow disabled"),
            (false, None),
        );
    }

    #[test]
    fn workflow_dispatch_result_preserves_non_dropped_reply_errors() {
        let error = workflow_dispatch_result(Err(
            ActorLifecycleError::Destroying.build(),
        ))
        .expect_err("non-dropped reply errors should be preserved");
        let error = ActorError::extract(&error);
        assert_eq!(error.group(), "actor");
        assert_eq!(error.code(), "destroying");
    }

    // The local host never streams a request body, so its stream
    // stand-in yields no chunk: the buffered body is the prefix
    // alone.
    #[tokio::test]
    async fn non_streaming_factory_request_is_buffered_before_dispatch()
     {
        let request = Request::from_parts_with_stream(
            "POST",
            "/",
            HashMap::new(),
            b"prefix".to_vec(),
            Some(HttpRequestBodyStream),
        )
        .expect("build streaming request");

        let request =
            request.into_buffered().await.expect("buffer request");

        assert!(!request.has_body_stream());
        assert_eq!(request.into_body(), b"prefix");
    }

    #[test]
    fn authorization_bearer_token_accepts_case_insensitive_scheme_and_whitespace()
     {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            "bearer   test-token".parse().unwrap(),
        );

        assert_eq!(
            authorization_bearer_token(&headers),
            Some("test-token")
        );

        let map = HashMap::from([(
            http::header::AUTHORIZATION.as_str().to_owned(),
            "BEARER\ttest-token".to_owned(),
        )]);
        assert_eq!(
            authorization_bearer_token_map(&map),
            Some("test-token")
        );
    }

    #[tokio::test]
    async fn action_dispatch_timeout_returns_structured_error() {
        let error = with_action_dispatch_timeout(
            Duration::from_millis(1),
            async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok::<Vec<u8>, ActionDispatchError>(Vec::new())
            },
        )
        .await
        .expect_err("timeout should return an action dispatch error");

        assert_eq!(error.group, "actor");
        assert_eq!(error.code, "action_timed_out");
        assert_eq!(error.message, "Action timed out");
    }

    #[tokio::test]
    async fn framework_action_timeout_returns_structured_error() {
        let error = with_framework_action_timeout(
            Duration::from_millis(1),
            async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok::<(), anyhow::Error>(())
            },
        )
        .await
        .expect_err("timeout should return a framework error");
        let error = ActorError::extract(&error);

        assert_eq!(error.group(), "actor");
        assert_eq!(error.code(), "action_timed_out");
        assert_eq!(error.message(), "Action timed out");
    }

    #[test]
    fn request_encoding_reads_cbor_header() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "x-hamlet-actor-encoding",
            "cbor".parse().unwrap(),
        );

        assert_eq!(
            request_encoding(&headers),
            HttpResponseEncoding::Cbor
        );
    }

    #[test]
    fn normalize_actor_request_path_preserves_raw_root_paths() {
        assert!(is_actor_request_path("/request"));
        assert!(is_actor_request_path("/request/"));
        assert!(is_actor_request_path("/request/users/1"));
        assert!(is_actor_request_path("/request?foo=bar"));
        assert_eq!(normalize_actor_request_path("/request"), "/");
        assert_eq!(normalize_actor_request_path("/request/"), "/");
        assert_eq!(
            normalize_actor_request_path("/request/users/1"),
            "/users/1",
        );
        assert_eq!(
            normalize_actor_request_path("/request?foo=bar"),
            "?foo=bar"
        );
    }

    #[test]
    fn normalize_actor_request_path_does_not_mark_framework_routes_as_raw()
     {
        assert!(!is_actor_request_path("/"));
        assert!(!is_actor_request_path("/action/ping"));
        assert!(!is_actor_request_path("/requestfoo"));
        assert_eq!(normalize_actor_request_path("/"), "/");
        assert_eq!(
            normalize_actor_request_path("/action/ping"),
            "/action/ping"
        );
        assert_eq!(
            normalize_actor_request_path("/requestfoo"),
            "/requestfoo"
        );
    }
}
