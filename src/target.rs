//! Which documents a source file owns, and where they live.
//!
//! Routing is by path, not by content: the path is what decides the title and
//! the output locations, and a manifest that parses as something other than its
//! filename suggests is an authoring mistake rather than a routing instruction.
//! Parsing only decides whether there is anything to document at all.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Where GitHub requires sources to live. Used for discovery only, and not
/// configurable, because GitHub does not make it configurable either.
const ACTIONS_ROOT: &str = ".github/actions";
const WORKFLOWS_ROOT: &str = ".github/workflows";

/// Appended to `--docs-dir-target` when mirroring. Deliberately distinct from
/// the roots above: one is GitHub's layout, the other is the caller's.
const ACTIONS_SUBDIR: &str = "actions";
const WORKFLOWS_SUBDIR: &str = "workflows";

const DOC_EXTENSION: &str = "md";
const README: &str = "README.md";

/// The file names GitHub accepts for an action manifest.
const MANIFESTS: [&str; 2] = ["action.yml", "action.yaml"];

/// The extensions GitHub recognises for a workflow. Compared exactly rather
/// than case-insensitively, because GitHub itself is case-sensitive here: a
/// file named `CI.YML` is not a workflow.
const WORKFLOW_EXTENSIONS: [&str; 2] = ["yml", "yaml"];

/// What a source file is, and therefore what gets generated from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Action,
    Workflow,
}

/// A link from a generated document back to the file it was generated from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The source path, as displayed.
    pub target: String,
    /// The href, relative to the document.
    pub href: String,
}

/// One document generated from a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Where the document lives, relative to the repository root.
    pub path: PathBuf,
    /// How the document introduces itself, for one that does not sit beside
    /// its source. A README in the action's own directory needs no such link.
    pub link: Option<Link>,
    /// Whether the document carries a usage snippet.
    pub usage: bool,
}

/// A source file and everything generated from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub kind: Kind,
    /// The heading, and the name shown in the index.
    pub title: String,
    /// The source path, relative to the repository root.
    pub source: PathBuf,
    /// Every document to write. The first is the canonical one.
    pub plans: Vec<Plan>,
}

impl Target {
    /// Where the repository index links this target.
    ///
    /// The canonical document for an action is the README beside it, which is
    /// what someone browsing the source tree finds first.
    pub fn index_href(&self) -> String {
        self.plans
            .first()
            .map(|plan| slashed(&plan.path))
            .unwrap_or_default()
    }

    /// The path a caller puts in `uses:`.
    ///
    /// An action is referenced by the directory holding its manifest; a
    /// reusable workflow is referenced by the file itself, extension included.
    /// The asymmetry is GitHub's, not ours.
    pub fn uses_path(&self) -> Option<String> {
        match self.kind {
            Kind::Action => self.source.parent().map(slashed),
            Kind::Workflow => Some(slashed(&self.source)),
        }
    }
}

/// Decide what a path is and what it generates.
///
/// A document beside the source is always written. A mirrored copy under
/// `docs_dir` is written only when one was asked for: where a repository
/// publishes its documentation is a choice, and a tool that scatters files into
/// `docs/` uninvited is one that has to be argued with.
///
/// Returns `None` only for a path with no usable file name, which a hook runner
/// will never produce.
pub fn classify(source: &Path, docs_dir: Option<&Path>) -> Option<Target> {
    let (kind, title, beside) = if is_manifest(source) {
        let directory = source.parent()?;
        let title = directory.file_name()?.to_str()?.to_owned();
        (Kind::Action, title, directory.join(README))
    } else {
        let title = source.file_stem()?.to_str()?.to_owned();
        (Kind::Workflow, title, source.with_extension(DOC_EXTENSION))
    };

    // No link: a document sitting next to its source does not need to point at
    // something the reader is already looking at.
    //
    // Both kinds carry a usage snippet. Calling a reusable workflow means
    // knowing it is a job rather than a step, that the path names the file,
    // and that secrets and permissions need their own blocks — which is more
    // to remember than an action step, not less.
    let mut plans = vec![Plan {
        path: beside,
        link: None,
        usage: true,
    }];

    if let Some(root) = docs_dir {
        let subdirectory = match kind {
            Kind::Action => ACTIONS_SUBDIR,
            Kind::Workflow => WORKFLOWS_SUBDIR,
        };
        let path = root
            .join(subdirectory)
            .join(format!("{title}.{DOC_EXTENSION}"));

        plans.push(Plan {
            link: Some(link_to(source, &path)),
            path,
            usage: true,
        });
    }

    Some(Target {
        kind,
        title,
        source: source.to_path_buf(),
        plans,
    })
}

/// Every action manifest in the repository, relative to `root`.
///
/// The index has to list everything, not just what changed in this commit, so
/// this is the one place that discovers files rather than being told about them.
pub fn discover_actions(root: &Path) -> Result<Vec<PathBuf>> {
    let base = Path::new(ACTIONS_ROOT);
    let mut manifests = Vec::new();

    for name in names_in(&root.join(base))? {
        let directory = base.join(name);
        if !root.join(&directory).is_dir() {
            continue;
        }
        if let Some(manifest) = MANIFESTS
            .iter()
            .map(|manifest| directory.join(manifest))
            .find(|candidate| root.join(candidate).is_file())
        {
            manifests.push(manifest);
        }
    }

    Ok(manifests)
}

/// Every workflow file in the repository, relative to `root`.
///
/// Whether one is *reusable* is a question about its contents, so it is left to
/// the caller, which has to parse the file anyway.
pub fn discover_workflows(root: &Path) -> Result<Vec<PathBuf>> {
    let base = Path::new(WORKFLOWS_ROOT);

    Ok(names_in(&root.join(base))?
        .into_iter()
        .filter(|name| is_workflow_file(name))
        .map(|name| base.join(name))
        .filter(|path| root.join(path).is_file())
        .collect())
}

/// The sorted names of a directory's entries, or none if it does not exist.
///
/// A repository with no actions is ordinary, not an error. Sorting matters more
/// than it looks: `read_dir` yields entries in filesystem order, which differs
/// between machines, and an index that reorders itself per checkout would make
/// the hook rewrite the README depending on who ran it.
fn names_in(directory: &Path) -> Result<Vec<String>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("cannot read {}", directory.display()));
        }
    };

    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read {}", directory.display()))?;
        // A name that is not UTF-8 cannot appear in a Markdown link, and
        // nothing in a repository of workflows should have one.
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_owned());
        }
    }
    names.sort();

    Ok(names)
}

fn is_manifest(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(OsStr::to_str),
        Some("action.yml" | "action.yaml")
    )
}

fn is_workflow_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| WORKFLOW_EXTENSIONS.contains(&extension))
}

/// The href from a generated document back to its source.
///
/// Computed from the document's own depth rather than assumed. Now that the
/// documentation root is a caller's choice, a hard-coded `../../` would produce
/// links pointing outside the repository the first time someone passed
/// something other than a two-deep directory.
fn link_to(source: &Path, document: &Path) -> Link {
    let depth = document.components().count().saturating_sub(1);
    let mut href = "../".repeat(depth);
    href.push_str(&slashed(source));

    Link {
        target: slashed(source),
        href,
    }
}

/// A path as it appears in Markdown, which uses forward slashes everywhere.
fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = ".github/actions/pre-commit/action.yml";
    const WORKFLOW: &str = ".github/workflows/lint.yml";

    fn action(docs: Option<&str>) -> Target {
        classify(Path::new(MANIFEST), docs.map(Path::new)).unwrap()
    }

    fn workflow(docs: Option<&str>) -> Target {
        classify(Path::new(WORKFLOW), docs.map(Path::new)).unwrap()
    }

    #[test]
    fn an_action_is_titled_after_its_directory() {
        assert_eq!(action(None).kind, Kind::Action);
        assert_eq!(action(None).title, "pre-commit");
    }

    #[test]
    fn a_workflow_is_titled_after_its_file() {
        assert_eq!(workflow(None).kind, Kind::Workflow);
        assert_eq!(workflow(None).title, "lint");
    }

    #[test]
    fn only_the_document_beside_the_source_is_written_by_default() {
        assert_eq!(
            action(None).plans,
            [Plan {
                path: PathBuf::from(".github/actions/pre-commit/README.md"),
                link: None,
                usage: true,
            }]
        );
        assert_eq!(
            workflow(None).plans,
            [Plan {
                path: PathBuf::from(".github/workflows/lint.md"),
                link: None,
                usage: true,
            }]
        );
    }

    #[test]
    fn a_mirror_is_written_only_when_a_root_is_given() {
        assert_eq!(action(None).plans.len(), 1);
        assert_eq!(action(Some("docs")).plans.len(), 2);
    }

    #[test]
    fn the_mirror_links_back_to_the_source() {
        assert_eq!(
            action(Some("docs")).plans[1],
            Plan {
                path: PathBuf::from("docs/actions/pre-commit.md"),
                link: Some(Link {
                    target: ".github/actions/pre-commit/action.yml".to_owned(),
                    href: "../../.github/actions/pre-commit/action.yml".to_owned(),
                }),
                usage: true,
            }
        );
        assert_eq!(
            workflow(Some("docs")).plans[1].path,
            PathBuf::from("docs/workflows/lint.md")
        );
    }

    #[test]
    fn the_link_follows_the_depth_of_the_documentation_root() {
        let deep = action(Some("site/reference"));

        assert_eq!(
            deep.plans[1].path,
            PathBuf::from("site/reference/actions/pre-commit.md")
        );
        assert_eq!(
            deep.plans[1].link.as_ref().unwrap().href,
            "../../../.github/actions/pre-commit/action.yml"
        );
    }

    #[test]
    fn the_yaml_spelling_of_a_manifest_is_also_an_action() {
        let target = classify(Path::new(".github/actions/x/action.yaml"), None).unwrap();
        assert_eq!(target.kind, Kind::Action);
        assert_eq!(target.title, "x");
    }

    #[test]
    fn both_kinds_carry_a_usage_snippet() {
        let action = classify(Path::new(".github/actions/greet/action.yml"), None).unwrap();
        let workflow = classify(Path::new(".github/workflows/release.yml"), None).unwrap();

        assert!(action.plans.iter().all(|plan| plan.usage));
        assert!(workflow.plans.iter().all(|plan| plan.usage));
    }

    #[test]
    fn an_action_is_referenced_by_its_directory_and_a_workflow_by_its_file() {
        // A caller writes `uses: owner/repo/.github/actions/pre-commit@ref`
        // but `uses: owner/repo/.github/workflows/lint.yml@ref`. The extension
        // is GitHub's asymmetry, not an oversight here.
        assert_eq!(
            action(None).uses_path().as_deref(),
            Some(".github/actions/pre-commit")
        );
        assert_eq!(
            workflow(None).uses_path().as_deref(),
            Some(".github/workflows/lint.yml")
        );
    }

    #[test]
    fn the_index_links_the_document_beside_the_source() {
        // Unchanged by the mirror: the canonical document is the one a reader
        // browsing the source tree finds first.
        assert_eq!(
            action(Some("docs")).index_href(),
            ".github/actions/pre-commit/README.md"
        );
        assert_eq!(workflow(None).index_href(), ".github/workflows/lint.md");
    }

    fn repository() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();

        for (name, manifest) in [("zzz", "action.yml"), ("aaa", "action.yaml")] {
            let directory = root.path().join(ACTIONS_ROOT).join(name);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(manifest), "runs:\n").unwrap();
        }
        // A directory with no manifest is not an action.
        fs::create_dir_all(root.path().join(ACTIONS_ROOT).join("mmm")).unwrap();

        let workflows = root.path().join(WORKFLOWS_ROOT);
        fs::create_dir_all(&workflows).unwrap();
        fs::write(workflows.join("b.yml"), "").unwrap();
        fs::write(workflows.join("a.yaml"), "").unwrap();
        fs::write(workflows.join("notes.md"), "").unwrap();

        root
    }

    #[test]
    fn actions_are_discovered_in_a_stable_order() {
        let root = repository();

        assert_eq!(
            discover_actions(root.path()).unwrap(),
            [
                PathBuf::from(".github/actions/aaa/action.yaml"),
                PathBuf::from(".github/actions/zzz/action.yml"),
            ]
        );
    }

    #[test]
    fn workflows_are_discovered_in_a_stable_order() {
        let root = repository();

        assert_eq!(
            discover_workflows(root.path()).unwrap(),
            [
                PathBuf::from(".github/workflows/a.yaml"),
                PathBuf::from(".github/workflows/b.yml"),
            ]
        );
    }

    #[test]
    fn a_repository_with_nothing_to_document_is_not_an_error() {
        let root = tempfile::tempdir().unwrap();

        assert!(discover_actions(root.path()).unwrap().is_empty());
        assert!(discover_workflows(root.path()).unwrap().is_empty());
    }

    #[test]
    fn only_a_yaml_extension_makes_a_workflow_file() {
        assert!(is_workflow_file("ci.yml"));
        assert!(is_workflow_file("ci.yaml"));
        assert!(!is_workflow_file("notes.md"));
        assert!(!is_workflow_file("CI.YML"));
        // A dotfile is a name, not an extension.
        assert!(!is_workflow_file(".yml"));
    }
}
