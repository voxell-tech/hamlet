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
}
