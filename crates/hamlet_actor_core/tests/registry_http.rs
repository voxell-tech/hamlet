// This file was changed for Hamlet. See NOTICE in this crate.
mod moved_tests {
    use std::collections::HashMap;

    use crate::actor::messages::Request;
    use crate::host::config::HttpRequestBodyStream;

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
}
