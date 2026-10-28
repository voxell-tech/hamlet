//! What the bazaar's server answers the seller's desk in the sell
//! scenes: the chief's listings with their versions, the terms for
//! selling and the files of the versions shown.

use serde_json::{Value, json};

/// The chief's listings, as `GET /v1/me/listings` answers: none, or
/// an agent with a version at each state, a hamlet and an agent being
/// checked.
pub(super) fn listings(none: bool) -> Value {
    if none {
        return json!({"items": [], "total": 0});
    }
    let details = |title: &str,
                   summary: &str,
                   cents: u32,
                   tags: &[&str]| {
        json!({"title": title, "summary": summary,
            "description": "Lights the lamp at dusk, logs every ship that \
                passes, and rings the bell when fog rolls in.",
            "priceCents": cents, "tags": tags, "coverUrl": null})
    };
    let keeper = details(
        "Lighthouse Keeper",
        "Keeps watch at night",
        199,
        &["night", "guard"],
    );
    let version = |version: &str,
                   state: &str,
                   at: &str,
                   files: u32| {
        json!({"version": version, "state": state, "createdAt": at,
            "size": 54_210, "fileCount": files, "details": keeper})
    };
    let mut rejected =
        version("0.0.2", "rejected", "2026-10-24T18:40:51Z", 4);
    rejected["reasons"] = json!(["Agent 5 (Safety): the tool \
                                  \"log\" sends ship records to \
                                  an outside address."]);
    let harbor =
        details("Harbor Town", "A fishing village", 0, &["village"]);
    let ferry =
        details("Ferryman", "Carries agents across", 99, &["river"]);
    json!({"items": [
        {"id": "l_keeper", "kind": "agent", "title": "Lighthouse Keeper",
            "versions": [
                version("0.0.1", "unverified", "2026-10-20T09:14:02Z", 4),
                rejected,
                version("0.0.3", "verified", "2026-10-26T11:35:42Z", 3),
                version("0.0.4", "draft", "2026-10-28T14:02:11Z", 4),
            ]},
        {"id": "l_harbor", "kind": "hamlet", "title": "Harbor Town",
            "versions": [{"version": "1.2.0", "state": "unverified",
                "createdAt": "2026-10-22T08:00:00Z", "size": 120_400,
                "fileCount": 1, "details": harbor}]},
        {"id": "l_ferry", "kind": "agent", "title": "Ferryman",
            "versions": [{"version": "0.0.2", "state": "checking",
                "createdAt": "2026-10-28T13:50:10Z", "size": 8_120,
                "fileCount": 1, "details": ferry}]},
    ], "total": 3})
}

/// The terms for selling, as `GET /v1/sell/config` answers.
pub(super) fn fees() -> Value {
    json!({"kinds": [
        {"kind": "agent", "available": true, "verifyCents": 150,
            "maxBytes": 10_485_760},
        {"kind": "hamlet", "available": true, "verifyCents": 400,
            "maxBytes": 52_428_800},
        {"kind": "plugin", "available": false, "verifyCents": 350,
            "maxBytes": 10_485_760},
    ], "updateVerifyCents": 99, "minPriceCents": 99})
}

/// The files of the Lighthouse Keeper's versions, each against the
/// version before: the rejected one sends its log away, which the
/// verified one takes out.
pub(super) fn files() -> Vec<(String, String, Value)> {
    let toml_a = "name = \"Lighthouse Keeper\"\nmodel = \
                  \"fast\"\ntools = [\"bell\", \"log\"]";
    let toml_b = "name = \"Lighthouse Keeper\"\nmodel = \
                  \"fast\"\ntools = [\"bell\", \"log\", \"lamp\"]";
    let day = "Sleep through the day.\nWake at dusk.";
    let night = "Keep the lamp lit until dawn.\nLog every ship you \
                 see, with its flag.\nRing the bell three times in \
                 fog.";
    let file = |path: &str,
                change: &str,
                text: &str,
                before: Option<&str>| {
        json!({"path": path, "size": text.len(), "executable": false,
            "change": change, "text": text, "previousText": before})
    };
    let same =
        |path: &str, text: &str| file(path, "same", text, Some(text));
    let (bell_0, bell_1) = (
        "{ \"name\": \"bell\", \"rings\": 0 }",
        "{ \"name\": \"bell\", \"rings\": 1 }",
    );
    let log =
        "{ \"name\": \"log\", \"to\": \"https://ships.example\" }";
    let older = vec![
        same("agent.toml", toml_a),
        same(
            "prompts/day.md",
            "Sleep through the day.\nWake at noon.\nWatch the sea.",
        ),
        same("tools/bell.json", bell_0),
        same("tools/log.json", log),
    ];
    let newest = vec![
        file("agent.toml", "changed", toml_b, Some(toml_a)),
        same("prompts/day.md", day),
        file("prompts/night.md", "added", night, None),
        file(
            "tools/bell.json",
            "changed",
            "{ \"name\": \"bell\", \"rings\": 3 }",
            Some("{ \"name\": \"bell\", \"rings\": 1 }"),
        ),
    ];
    let checked = vec![
        same("agent.toml", toml_a),
        file(
            "prompts/day.md",
            "changed",
            day,
            Some(
                "Sleep through the day.\nWake at noon.\nWatch the \
                 sea.",
            ),
        ),
        file("tools/bell.json", "changed", bell_1, Some(bell_0)),
        file("tools/log.json", "removed", "", Some(log)),
    ];
    [
        ("0.0.2", "0.0.1", older),
        ("0.0.3", "0.0.2", checked),
        ("0.0.4", "0.0.3", newest),
    ]
    .into_iter()
    .map(|(version, against, files)| {
        let answer =
            json!({"version": version, "against": against, "files": files});
        ("l_keeper".into(), version.into(), answer)
    })
    .collect()
}
