//! A real scratch repository for the review panel's gallery scenes.

use bevy::prelude::*;
use hamlet_rules::review::Comparison;
use hamlet_testing::Scratch;
use hamlet_ui::screens::review;
use hamlet_ui::widgets::diff::Layout;

use super::Scene;

#[derive(Resource)]
struct Repository {
    _folder: Scratch,
}

pub(super) fn setup(world: &mut World) {
    let scene = *world.resource::<Scene>();
    if !matches!(
        scene,
        Scene::Review | Scene::ReviewSplit | Scene::ReviewRevisions
    ) {
        return;
    }
    let root = Scratch::new("review-gallery");
    let git = |args: &[&str]| {
        let output =
            hamlet_testing::git(&root).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.name", "Gallery"]);
    git(&["config", "user.email", "gallery@example.invalid"]);
    let prefix = (1..=12)
        .map(|i| format!("// Route configuration {i}\n"))
        .collect::<String>();
    let base = format!(
        "{prefix}pub fn route(path: &str) -> bool {{\n    let \
         allowed = path.starts_with(\"/api\");\n    allowed\n}}\n"
    );
    std::fs::write(root.join("routes.rs"), &base).unwrap();
    std::fs::write(
        root.join("README.md"),
        "# Routes\n\nHTTP routes for the hamlet.\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "Initial routes"]);
    let updated = base.replace(
        "    let allowed = path.starts_with(\"/api\");\n    allowed",
        "    if path == \"/health\" {\n        return true;\n    \
         }\n    path.starts_with(\"/api/\")",
    );
    std::fs::write(root.join("routes.rs"), &updated).unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "Allow health checks"]);
    std::fs::write(
        root.join("routes.rs"),
        updated.replace("\"/health\"", "\"/ready\""),
    )
    .unwrap();
    git(&["add", "."]);
    std::fs::write(
        root.join("routes.rs"),
        base.replace(
            "    let allowed = path.starts_with(\"/api\");\n    \
             allowed",
            "    if path == \"/ready\" {\n        return true;\n    \
             }\n    let prefix = \"/api/v2/\";\n    \
             path.starts_with(prefix)",
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("NOTES.md"),
        "# Review checklist\n\n- Health checks bypass routing.\n- \
         Match complete path segments.\n",
    )
    .unwrap();
    let comparison = if scene == Scene::ReviewRevisions {
        Comparison::Revisions {
            base: "HEAD~1".into(),
            target: "HEAD".into(),
            merge_base: true,
        }
    } else {
        Comparison::Unstaged
    };
    review::inspect(world, root.to_path_buf(), comparison);
    if scene == Scene::ReviewSplit {
        review::set_layout(world, Layout::Split);
    }
    world.insert_resource(Repository { _folder: root });
}
