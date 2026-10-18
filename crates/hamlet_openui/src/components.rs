//! The components a program may use, each with its props in
//! positional order. The list is the one source of what the language
//! offers a panel.

use openui_lang::{ComponentDef, Param, Type};

/// The root component, which a program without `root` falls back to.
pub const ROOT: &str = "Column";

/// Every component.
pub fn all() -> Vec<ComponentDef> {
    vec![
        container("Column", "Children from top to bottom"),
        container("Row", "Children from left to right, wrapping"),
        container(
            "Scroll",
            "Children from top to bottom that scroll when they do \
             not fit",
        ),
        text("Label", "Body text"),
        text("Heading", "The name over a part of the panel"),
        text("Note", "Small dim words"),
        ComponentDef::new("Icon", "An icon of the panels, by name")
            .param(Param::required("name", Type::String)),
        ComponentDef::new("Button", "A button")
            .param(Param::required("label", Type::String))
            .param(Param::optional("action", Type::action())),
        ComponentDef::new("Checkbox", "A tick and its label")
            .param(Param::required("label", Type::String))
            .param(
                Param::optional("checked", Type::Boolean)
                    .bindable()
                    .with_default(false),
            )
            .param(Param::optional("action", Type::action())),
        ComponentDef::new("TextField", "A line of text to type in")
            .param(
                Param::optional("text", Type::String)
                    .bindable()
                    .with_default(""),
            ),
        ComponentDef::new(
            "TextArea",
            "Several lines of text to type in",
        )
        .param(
            Param::optional("text", Type::String)
                .bindable()
                .with_default(""),
        ),
        text("Markdown", "Markdown text, drawn and not editable"),
        ComponentDef::new("Divider", "A hairline"),
    ]
}

fn container(name: &str, description: &str) -> ComponentDef {
    ComponentDef::new(name, description).param(Param::required(
        "children",
        Type::list(Type::component()),
    ))
}

fn text(name: &str, description: &str) -> ComponentDef {
    ComponentDef::new(name, description)
        .param(Param::required("text", Type::String))
}
