//! A model call a plugin makes: with no tools, on the budget of the
//! home's first hamlet, charged under the plugin's name, and cut off
//! where its reply passes the tokens it may have.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use hamlet_model::provider::{CallRecord, Delta, Provider};
use hamlet_rules::budget;
use hamlet_rules::hamlet::Hamlet;
use serde_json::{Value, json};
use tokio::sync::oneshot;

/// The characters a token is counted as where a reply is cut off.
const CHARS_PER_TOKEN: usize = 4;

/// Who says a message of a model call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    User,
    Assistant,
}

/// A model call a plugin asks for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ModelCall {
    /// The id of the plugin, which the call is charged to.
    pub plugin: String,
    pub system: String,
    pub messages: Vec<(Role, String)>,
    /// The most tokens of the reply.
    pub reply_tokens: u32,
}

impl ModelCall {
    /// The messages the model is sent.
    pub fn request(&self) -> Vec<Value> {
        let said = self.messages.iter().map(|(role, text)| {
            let role = match role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            json!({"role": role, "content": text})
        });
        std::iter::once(
            json!({"role": "system", "content": self.system}),
        )
        .chain(said)
        .collect()
    }

    /// The tokens it is counted as, the reply it may have among them:
    /// four characters a token.
    pub fn tokens(&self) -> u64 {
        budget::estimate(&self.request(), &[], &Value::Null)
            + u64::from(self.reply_tokens)
    }

    /// The name it is charged under in the hamlet's books.
    pub fn payer(&self) -> String {
        format!("plugin {}", self.plugin)
    }
}

/// Makes `call` with `provider`, charged to the first hamlet of
/// `home`: the text of the reply, or why there is none. It is refused
/// where the hamlet has spent its budget.
pub async fn call_model(
    provider: Arc<dyn Provider>,
    home: PathBuf,
    call: ModelCall,
) -> Result<String, String> {
    let hamlet = Hamlet::new(home);
    let payer = call.payer();
    let task = budget::new_task(&payer);
    if let Some(why) = hamlet.stopped(&task) {
        return Err(why);
    }
    let model = hamlet.model_of(&payer);
    let request = call.request();
    let most = call.reply_tokens as usize * CHARS_PER_TOKEN;
    let started = Instant::now();
    let mut text = String::new();
    let (cut, cut_off) = oneshot::channel::<()>();
    let mut cut = Some(cut);
    let done = {
        let mut on_delta = |delta: Delta<'_>| {
            if let Delta::Text(more) = delta {
                text.push_str(more);
                if text.len() > most
                    && let Some(cut) = cut.take()
                {
                    let _ = cut.send(());
                }
            }
        };
        let asked = provider.complete(
            &request,
            &[],
            model.as_deref(),
            &mut on_delta,
        );
        tokio::select! {
            done = asked => Some(done),
            _ = cut_off => None,
        }
    };
    let (reply, record) = match done {
        Some(Ok(done)) => (
            done.message["content"]
                .as_str()
                .unwrap_or_default()
                .into(),
            done.call,
        ),
        Some(Err(e)) => {
            return Err(format!("the model did not answer: {e:#}"));
        }
        // Cut off: the provider's count of what it used never came.
        None => (
            text,
            CallRecord {
                model: model.unwrap_or_default(),
                duration_ms: started.elapsed().as_millis() as u64,
                finish_reason: Some("length".into()),
                ..CallRecord::default()
            },
        ),
    };
    let estimate =
        budget::estimate(&request, &[], &json!({"content": reply}));
    hamlet
        .spend(&payer, &task, &record, estimate)
        .map_err(|e| {
            format!("the call could not be charged: {e:#}")
        })?;
    Ok(cut_to(reply, most))
}

/// `text` in at most `most` bytes.
fn cut_to(mut text: String, most: usize) -> String {
    text.truncate(text.floor_char_boundary(most));
    text
}

#[cfg(test)]
mod tests {
    use hamlet_model::provider::Replay;
    use hamlet_testing::Scratch;

    use super::*;

    #[tokio::test]
    async fn a_plugins_call_is_charged_to_the_hamlet_and_cut_to_its_tokens()
     {
        let home = Scratch::new("model-call");
        let replies = ["high water at noon", &"x".repeat(100)].map(
            |reply| json!({"role": "assistant", "content": reply}),
        );
        let provider = Arc::new(Replay::new(replies.to_vec()));
        let call = ModelCall {
            plugin: "tides".into(),
            system: "Be brief.".into(),
            messages: vec![(
                Role::User,
                "When is high water?".into(),
            )],
            reply_tokens: 8,
        };

        let said = call_model(
            provider.clone(),
            home.to_path_buf(),
            call.clone(),
        )
        .await;
        assert_eq!(said.as_deref(), Ok("high water at noon"));
        let long = call_model(
            provider.clone(),
            home.to_path_buf(),
            call.clone(),
        )
        .await
        .unwrap();
        assert_eq!(long.len(), 32);

        let report = Hamlet::new(home.to_path_buf()).report("own");
        assert!(report.hamlet.spent.tokens > 0);
        let limits = hamlet_home::limits::path(&home);
        std::fs::write(limits, "[budget]\ncost = 0.0\n").unwrap();
        let spent =
            call_model(provider, home.to_path_buf(), call).await;
        assert!(spent.unwrap_err().contains("spent its budget"));
    }
}
