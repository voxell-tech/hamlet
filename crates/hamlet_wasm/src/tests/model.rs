use hamlet_agents_api::model::Model;

use super::acts::{does, open, probe};
use crate::fixture::home::Scratch;
use crate::interface::exports::hamlet::plugin::plugin::{
    Intent, ModelMessage, ModelRequest, ModelRole,
};
use crate::limits::Limits;

fn ask(text: &str) -> Vec<Intent> {
    vec![Intent::ModelCall(ModelRequest {
        id: 1,
        system: "Be brief.".into(),
        messages: vec![ModelMessage {
            role: ModelRole::User,
            text: text.into(),
        }],
        max_tokens: Some(64),
    })]
}

/// The lines of the home's books.
fn charged(scratch: &Scratch) -> Vec<String> {
    let Ok(days) = std::fs::read_dir(scratch.home().join("spend"))
    else {
        return Vec::new();
    };
    days.filter_map(Result::ok)
        .filter_map(|day| std::fs::read_to_string(day.path()).ok())
        .flat_map(|text| {
            text.lines().map(str::to_string).collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn a_model_call_is_charged_under_the_plugins_name_within_its_tokens()
{
    let scratch = Scratch::new("model_call");
    let mut app = open(
        &scratch,
        probe().asking(&["model.call"]).granted(&["model.call"]),
    );
    app.insert_resource(Model::answering(&["noon", "never"]))
        .insert_resource(Limits {
            model_call_tokens: 200,
            ..Limits::default().patient()
        });

    does(&mut app, ask("When is high water?"));
    app.update();
    let books = charged(&scratch);
    assert_eq!(books.len(), 1);
    assert!(books[0].contains("\"agent\":\"plugin probe\""));

    does(&mut app, ask(&"tides ".repeat(200)));
    app.update();
    assert_eq!(charged(&scratch).len(), 1);
}
