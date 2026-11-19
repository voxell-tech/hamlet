//! What a model is given of an MCP tool's result: its text without
//! the secrets, cut as pi cuts a read, and its images when the model
//! takes them.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hamlet_protocol::client::ImageId;
use hamlet_sandbox::scrub::scrub_text;
use rmcp::model::{CallToolResult, ContentBlock, ResourceContents};
use serde_json::{Map, Value};

use crate::attachments::images::fitting::keep_fitted;
use crate::attachments::images::{Format, Unusable};
use crate::tools::builtin::truncate::{format_size, truncate_head};
use crate::tools::builtin::{Outcome, Sight};

/// The outcome of `result`, without `secrets`, with the images the
/// model of `sight` takes.
pub(super) async fn outcome(
    result: CallToolResult,
    secrets: &[String],
    sight: &Sight,
) -> Outcome {
    let mut parts = Vec::new();
    let mut images = Vec::new();
    for block in result.content {
        match block {
            ContentBlock::Text(text) => parts.push(text.text),
            ContentBlock::Image(image) => {
                match seen(&image.data, sight).await {
                    Ok(id) => images.push(id),
                    Err(why) => parts.push(format!(
                        "[an image ({}) left out: {why}]",
                        image.mime_type
                    )),
                }
            }
            ContentBlock::Audio(audio) => parts.push(format!(
                "[audio ({}) left out: models are given no sound]",
                audio.mime_type
            )),
            ContentBlock::Resource(embedded) => {
                parts.push(match embedded.resource {
                    ResourceContents::TextResourceContents {
                        text,
                        ..
                    } => text,
                    ResourceContents::BlobResourceContents {
                        uri,
                        mime_type,
                        ..
                    } => format!(
                        "[the content of {uri} ({}) left out]",
                        mime_type.as_deref().unwrap_or("binary")
                    ),
                    _ => continue,
                });
            }
            ContentBlock::ResourceLink(link) => {
                parts.push(format!("[{}: {}]", link.name, link.uri));
            }
            _ => {}
        }
    }
    // A server that gives structured content alone is read as JSON.
    if parts.is_empty()
        && images.is_empty()
        && let Some(structured) = result.structured_content
    {
        parts.push(structured.to_string());
    }
    let whole = scrub_text(secrets, &parts.join("\n"));
    let (mut text, truncation) = truncate_head(&whole);
    let mut details = Map::new();
    if truncation.truncated {
        text.push_str(&format!(
            "\n\n[The result is {}; only its first {} were kept.]",
            format_size(truncation.total_bytes),
            format_size(truncation.output_bytes),
        ));
        details.insert("truncation".into(), truncation.json());
    }
    let error = result.is_error == Some(true);
    if error && text.is_empty() {
        text = "The tool failed and said nothing more.".into();
    }
    Outcome {
        error,
        details: Some(details),
        images,
        ..Outcome::text(text)
    }
}

/// Why an image of a result is left out.
#[derive(Debug, thiserror::Error)]
enum LeftOut {
    #[error("the model takes no images")]
    Blind,
    #[error("it is not base64")]
    NotBase64,
    #[error(transparent)]
    Unusable(#[from] Unusable),
    #[error("it could not be kept: {0:#}")]
    Unkept(anyhow::Error),
}

/// The image of the base64 `data`, kept for the model of `sight`.
async fn seen(data: &str, sight: &Sight) -> Result<ImageId, LeftOut> {
    let limits = sight.limits.ok_or(LeftOut::Blind)?;
    let bytes =
        STANDARD.decode(data).map_err(|_| LeftOut::NotBase64)?;
    let format =
        Format::detect(&bytes).ok_or(Unusable::NotAnImage)?;
    let (id, _) =
        keep_fitted(sight.folder.clone(), bytes, format, limits)
            .await
            .map_err(LeftOut::Unkept)??;
    Ok(id)
}

/// The details of a result that name where it came from.
pub(super) fn origin(
    server: &str,
    tool: &str,
    policy: &str,
    degraded: &Value,
) -> Map<String, Value> {
    Map::from_iter([
        ("mcpServer".into(), server.into()),
        ("mcpTool".into(), tool.into()),
        ("policy".into(), policy.into()),
        ("degraded".into(), degraded.clone()),
    ])
}

#[cfg(test)]
mod tests {
    use rmcp::model::ContentBlock as Content;

    use super::*;

    #[tokio::test]
    async fn a_result_is_its_text_without_secrets_and_an_error_when_said()
     {
        let mut result = CallToolResult::error(vec![
            Content::text("token a-long-secret"),
            Content::image("AAAA", "image/png"),
            Content::text("two"),
        ]);
        result.structured_content = Some(serde_json::json!({"n": 1}));
        let outcome = outcome(
            result,
            &["a-long-secret".to_owned()],
            &Sight::blind(),
        )
        .await;
        assert!(outcome.error);
        assert_eq!(
            outcome.text,
            "token [REDACTED]\n[an image (image/png) left out: the \
             model takes no images]\ntwo"
        );
    }
}
