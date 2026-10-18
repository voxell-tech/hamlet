use super::parse::{Block, Kind, Span, Style, parse};

fn span(text: &str, style: Style) -> Span {
    Span {
        text: text.into(),
        style,
    }
}

fn plain(text: &str) -> Span {
    span(text, Style::default())
}

#[test]
fn headings_emphasis_lists_code_and_rules_become_blocks() {
    let blocks = parse(
        "# Title\n\nSome *soft*, **loud**, `code` and [a \
         link](https://example.org).\n\n1. one\n2. two\n   - \
         nested\n\n---\n\n```\nlet x;\n```\n",
    );

    let item = |indent, mark: &str, text| Block {
        indent,
        mark: Some(mark.into()),
        kind: Kind::Text(vec![plain(text)]),
    };
    assert_eq!(
        blocks,
        [
            Block {
                indent: 0,
                mark: None,
                kind: Kind::Heading(1, vec![plain("Title")]),
            },
            Block {
                indent: 0,
                mark: None,
                kind: Kind::Text(vec![
                    plain("Some "),
                    span(
                        "soft",
                        Style {
                            italic: true,
                            ..Style::default()
                        }
                    ),
                    plain(", "),
                    span(
                        "loud",
                        Style {
                            bold: true,
                            ..Style::default()
                        }
                    ),
                    plain(", "),
                    span(
                        "code",
                        Style {
                            code: true,
                            ..Style::default()
                        }
                    ),
                    plain(" and a link."),
                ]),
            },
            item(1, "1.", "one"),
            item(1, "2.", "two"),
            item(2, "\u{2022}", "nested"),
            Block {
                indent: 0,
                mark: None,
                kind: Kind::Rule,
            },
            Block {
                indent: 0,
                mark: None,
                kind: Kind::Code("let x;".into()),
            },
        ]
    );
}
