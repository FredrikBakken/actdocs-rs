//! Which documents a source file owns, and where they live.
//!
//! Routing is by path, not by content: the path is what decides the title and
//! the output locations, and a manifest that parses as something other than its
//! filename suggests is an authoring mistake rather than a routing instruction.
//! Parsing only decides whether there is anything to document at all.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

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

/// The other name a directory can introduce itself with. Offered alongside
/// `README.md` because static site generators look for this one, while GitHub
/// renders the former when someone browses the tree.
const INDEX: &str = "index.md";

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

/// This names what becomes of the document *beside the source*, and nothing
/// else: the mirrored copy is `--docs-dir-target`'s business and is written
/// whenever a root is named. So `Beside` together with a documentation root
/// means both, and `DocsDir` is how you ask for the mirror alone.
///
/// Only workflows have the choice. An action already owns a directory, so its
/// README sits alone in it; every workflow in a repository shares one
/// directory, and a `.md` beside each `.yml` doubles the length of the one
/// listing people actually scroll through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Placement {
    /// `.github/workflows/lint.md`, next to the workflow it describes — plus
    /// the mirror, where a documentation root was named.
    #[default]
    Beside,
    /// Under the documentation root and nowhere else, leaving the workflow
    /// directory to workflows.
    DocsDir,
}

/// The shape of a mirrored document under the documentation root.
///
/// Only the mirror is affected. The document beside the source keeps GitHub's
/// layout, which is not ours to rearrange: an action's README belongs in the
/// directory GitHub already gave it.
///
/// Spelled as one word rather than a table, so that the entry file cannot be
/// named where there is no directory to put it in: `flat` has no file name to
/// choose — the document *is* `<name>.md`. A setting that accepted one anyway
/// would have to either ignore it or quietly reinterpret the layout, and this
/// tool reports rather than guesses everywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    /// `docs/actions/<name>.md`. One file, and nowhere to put anything beside
    /// it.
    #[default]
    Flat,
    /// `docs/actions/<name>/README.md`, so that screenshots, diagrams and
    /// sub-pages have somewhere to live next to the document they belong to.
    /// `README.md` is the name GitHub renders when someone browses the tree.
    Directory,
    /// `docs/actions/<name>/index.md`: the same shape, named the way a static
    /// site generator looks for it rather than the way GitHub does.
    DirectoryIndex,
}

impl Layout {
    /// Where a mirror of `title` lands under `root`.
    fn mirror(self, root: &Path, subdirectory: &str, title: &str) -> PathBuf {
        let directory = root.join(subdirectory);
        match self {
            Self::Flat => directory.join(format!("{title}.{DOC_EXTENSION}")),
            Self::Directory => directory.join(title).join(README),
            Self::DirectoryIndex => directory.join(title).join(INDEX),
        }
    }

    /// Every shape a mirror could take, so that the ones not chosen can be
    /// reported. Listed here rather than at the call site because a variant
    /// added later must not silently go unmentioned.
    const ALL: [Self; 3] = [Self::Flat, Self::Directory, Self::DirectoryIndex];
}

/// Which layout applies to a given source, once the per-kind and per-target
/// overrides have had their say.
///
/// Resolution runs from the most specific statement to the least: a rule naming
/// one action beats a rule about actions, which beats the setting for the
/// repository. The ordering is the same one the configuration layers use, for
/// the same reason — the more narrowly a value was stated, the more deliberate
/// it was.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layouts {
    /// The repository-wide default, where nothing more specific applies.
    pub default: Layout,
    /// Every action, unless one of them is named below.
    pub actions: Option<Layout>,
    /// Every workflow, unless one of them is named below.
    pub workflows: Option<Layout>,
    /// One named action, by directory name.
    pub action: BTreeMap<String, Layout>,
    /// One named workflow, by file stem.
    pub workflow: BTreeMap<String, Layout>,
}

/// Accepts the one-word form as well as the table, so a repository that wants a
/// single shape writes `docs-dir-layout = "directory"` and never learns the
/// rest of this exists.
impl<'de> Deserialize<'de> for Layouts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "kebab-case", deny_unknown_fields)]
        struct Table {
            default: Option<Layout>,
            actions: Option<Layout>,
            workflows: Option<Layout>,
            #[serde(default)]
            action: BTreeMap<String, Layout>,
            #[serde(default)]
            workflow: BTreeMap<String, Layout>,
        }

        // Tried before the one-word form only because a table is the more
        // specific shape. The two cannot be confused: a string never matches
        // the table, and the table never matches a string.
        //
        // Named for the setting rather than for the mechanism, because serde
        // puts this name in the error a misspelling produces.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum DocsDirLayout {
            Table(Table),
            Uniform(Layout),
        }

        Ok(match DocsDirLayout::deserialize(deserializer)? {
            DocsDirLayout::Uniform(layout) => Self::uniform(layout),
            DocsDirLayout::Table(table) => Self {
                default: table.default.unwrap_or_default(),
                actions: table.actions,
                workflows: table.workflows,
                action: table.action,
                workflow: table.workflow,
            },
        })
    }
}

impl Layouts {
    /// Every layout as a single word, for a repository that wants one shape.
    #[must_use]
    pub fn uniform(layout: Layout) -> Self {
        Self {
            default: layout,
            ..Self::default()
        }
    }

    /// The layout for one source, most specific rule first.
    #[must_use]
    pub fn resolve(&self, kind: Kind, title: &str) -> Layout {
        let (named, all) = match kind {
            Kind::Action => (&self.action, self.actions),
            Kind::Workflow => (&self.workflow, self.workflows),
        };

        named.get(title).copied().or(all).unwrap_or(self.default)
    }

    /// Whether any rule could put a document somewhere the default would not.
    /// Used to decide whether looking for stranded documents is worth the
    /// filesystem calls.
    #[must_use]
    pub fn is_uniform(&self) -> bool {
        self.actions.is_none()
            && self.workflows.is_none()
            && self.action.is_empty()
            && self.workflow.is_empty()
    }
}

/// One document generated from a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Where the document lives, relative to the repository root.
    pub path: PathBuf,
    /// Whether the document introduces itself with a link back to its source.
    /// A README in the action's own directory needs no such link; a copy
    /// published elsewhere does. Where that link points depends on the
    /// repository it is published from, so it is settled in `sync`.
    pub source_link: bool,
    /// Whether the document carries a usage snippet.
    pub usage: bool,
}

/// Why a document is no longer written, so that the diagnostic can say which
/// setting stranded it rather than guessing at one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Workflow documents were configured for the documentation root.
    Placement,
    /// The mirror's layout changed, so the previous shape is stale.
    Layout,
}

impl Reason {
    pub fn explanation(self) -> &'static str {
        match self {
            Self::Placement => "workflow documents are configured for docs-dir",
            Self::Layout => "the documentation layout changed",
        }
    }
}

/// A document a differently configured run would have written, and this one
/// will not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    pub path: PathBuf,
    pub reason: Reason,
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
    /// Documents a different setting would have written, and this one will not.
    /// Reported rather than removed: everything outside the markers is
    /// hand-written, and a generator that deletes prose it did not produce is
    /// one nobody trusts twice.
    pub orphans: Vec<Orphan>,
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

    /// The source path as it is written in a link.
    pub fn source_path(&self) -> String {
        slashed(&self.source)
    }
}

/// Decide what a path is and what it generates.
///
/// A mirrored copy under `docs_dir` is written only when one was asked for:
/// where a repository publishes its documentation is a choice, and a tool that
/// scatters files into `docs/` uninvited is one that has to be argued with.
///
/// Returns `None` only for a path with no usable file name, which a hook runner
/// will never produce.
pub fn classify(
    source: &Path,
    docs_dir: Option<&Path>,
    workflows: Placement,
    layouts: &Layouts,
) -> Option<Target> {
    let (kind, title, beside) = if is_manifest(source) {
        let directory = source.parent()?;
        let title = directory.file_name()?.to_str()?.to_owned();
        (Kind::Action, title, directory.join(README))
    } else {
        let title = source.file_stem()?.to_str()?.to_owned();
        (Kind::Workflow, title, source.with_extension(DOC_EXTENSION))
    };

    let layout = layouts.resolve(kind, &title);
    let subdirectory = match kind {
        Kind::Action => ACTIONS_SUBDIR,
        Kind::Workflow => WORKFLOWS_SUBDIR,
    };

    let mirror = docs_dir.map(|root| Plan {
        path: layout.mirror(root, subdirectory, &title),
        source_link: true,
        usage: true,
    });

    // Displaces a workflow only, and only where there is somewhere for it to
    // go: dropping the document beside the source with no mirror to replace it
    // would document the file nowhere at all.
    let displaced = kind == Kind::Workflow && workflows == Placement::DocsDir && mirror.is_some();

    // No link on the document beside the source: it does not need to point at
    // something the reader is already looking at.
    //
    // Both kinds carry a usage snippet. Calling a reusable workflow means
    // knowing it is a job rather than a step, that the path names the file,
    // and that secrets and permissions need their own blocks — which is more
    // to remember than an action step, not less.
    let mut plans = Vec::new();
    if !displaced {
        plans.push(Plan {
            path: beside.clone(),
            source_link: false,
            usage: true,
        });
    }
    plans.extend(mirror);

    // Every shape this mirror could have taken but did not. Listing them all
    // rather than only the previous one means a repository that has changed
    // its mind twice still hears about both files it left behind.
    let mut orphans = Vec::new();
    if displaced {
        orphans.push(Orphan {
            path: beside,
            reason: Reason::Placement,
        });
    }
    if let Some(root) = docs_dir {
        let written = layout.mirror(root, subdirectory, &title);
        orphans.extend(
            Layout::ALL
                .into_iter()
                .map(|other| other.mirror(root, subdirectory, &title))
                .filter(|path| *path != written)
                .map(|path| Orphan {
                    path,
                    reason: Reason::Layout,
                }),
        );
    }

    Some(Target {
        kind,
        title,
        source: source.to_path_buf(),
        plans,
        orphans,
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
        laid_out(MANIFEST, docs, Placement::Beside, &Layouts::default())
    }

    fn workflow(docs: Option<&str>) -> Target {
        laid_out(WORKFLOW, docs, Placement::Beside, &Layouts::default())
    }

    /// A workflow whose document is configured to live under the docs root.
    fn displaced(docs: Option<&str>) -> Target {
        laid_out(WORKFLOW, docs, Placement::DocsDir, &Layouts::default())
    }

    fn laid_out(
        source: &str,
        docs: Option<&str>,
        placement: Placement,
        layouts: &Layouts,
    ) -> Target {
        classify(Path::new(source), docs.map(Path::new), placement, layouts).unwrap()
    }

    /// The paths a target would actually write.
    fn paths(target: &Target) -> Vec<PathBuf> {
        target.plans.iter().map(|plan| plan.path.clone()).collect()
    }

    /// The documents a target reports as stranded, for a given reason.
    fn orphans(target: &Target, reason: Reason) -> Vec<PathBuf> {
        target
            .orphans
            .iter()
            .filter(|orphan| orphan.reason == reason)
            .map(|orphan| orphan.path.clone())
            .collect()
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
                source_link: false,
                usage: true,
            }]
        );
        assert_eq!(
            workflow(None).plans,
            [Plan {
                path: PathBuf::from(".github/workflows/lint.md"),
                source_link: false,
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
                source_link: true,
                usage: true,
            }
        );
        assert_eq!(
            workflow(Some("docs")).plans[1].path,
            PathBuf::from("docs/workflows/lint.md")
        );
    }

    #[test]
    fn the_mirror_follows_the_depth_of_the_documentation_root() {
        let deep = action(Some("site/reference"));

        assert_eq!(
            deep.plans[1].path,
            PathBuf::from("site/reference/actions/pre-commit.md")
        );
    }

    #[test]
    fn the_yaml_spelling_of_a_manifest_is_also_an_action() {
        let target = laid_out(
            ".github/actions/x/action.yaml",
            None,
            Placement::Beside,
            &Layouts::default(),
        );
        assert_eq!(target.kind, Kind::Action);
        assert_eq!(target.title, "x");
    }

    #[test]
    fn both_kinds_carry_a_usage_snippet() {
        let action = laid_out(
            ".github/actions/greet/action.yml",
            None,
            Placement::Beside,
            &Layouts::default(),
        );
        let workflow = laid_out(
            ".github/workflows/release.yml",
            None,
            Placement::Beside,
            &Layouts::default(),
        );

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

    #[test]
    fn a_workflow_can_be_kept_out_of_the_workflow_directory() {
        assert_eq!(
            displaced(Some("docs")).plans,
            [Plan {
                path: PathBuf::from("docs/workflows/lint.md"),
                source_link: true,
                usage: true,
            }]
        );
    }

    #[test]
    fn the_displaced_document_is_named_rather_than_forgotten() {
        assert_eq!(
            orphans(&displaced(Some("docs")), Reason::Placement),
            [PathBuf::from(".github/workflows/lint.md")]
        );
        assert!(orphans(&workflow(Some("docs")), Reason::Placement).is_empty());
    }

    #[test]
    fn an_action_keeps_its_readme_whatever_workflows_do() {
        let target = laid_out(
            MANIFEST,
            Some("docs"),
            Placement::DocsDir,
            &Layouts::default(),
        );

        assert_eq!(
            target.plans[0].path,
            PathBuf::from(".github/actions/pre-commit/README.md")
        );
        assert!(orphans(&target, Reason::Placement).is_empty());
    }

    #[test]
    fn a_workflow_stays_put_when_there_is_nowhere_to_move_it() {
        // Rejected one layer up, in the configuration. Defended here anyway,
        // because a library that silently documented nothing would be a trap.
        assert_eq!(
            displaced(None).plans,
            [Plan {
                path: PathBuf::from(".github/workflows/lint.md"),
                source_link: false,
                usage: true,
            }]
        );
    }

    #[test]
    fn a_displaced_workflow_is_indexed_where_it_actually_lives() {
        assert_eq!(
            displaced(Some("docs")).index_href(),
            "docs/workflows/lint.md"
        );
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

    #[test]
    fn a_named_documentation_root_means_both_places_unless_told_otherwise() {
        // `beside` is not `beside only`. Naming a documentation root asks for
        // the mirror either way; this flag decides only whether the document
        // beside the source survives.
        assert_eq!(
            paths(&workflow(Some("docs"))),
            [
                PathBuf::from(".github/workflows/lint.md"),
                PathBuf::from("docs/workflows/lint.md"),
            ]
        );
    }

    #[test]
    fn a_directory_layout_gives_the_mirror_somewhere_to_keep_its_siblings() {
        let readme = laid_out(
            MANIFEST,
            Some("docs"),
            Placement::Beside,
            &Layouts::uniform(Layout::Directory),
        );
        let index = laid_out(
            MANIFEST,
            Some("docs"),
            Placement::Beside,
            &Layouts::uniform(Layout::DirectoryIndex),
        );

        assert_eq!(
            readme.plans[1].path,
            PathBuf::from("docs/actions/pre-commit/README.md")
        );
        assert_eq!(
            index.plans[1].path,
            PathBuf::from("docs/actions/pre-commit/index.md")
        );
    }

    #[test]
    fn the_document_beside_the_source_is_never_rearranged() {
        // GitHub's layout is not ours to change: only the mirror moves.
        for layout in [Layout::Flat, Layout::Directory, Layout::DirectoryIndex] {
            let layouts = Layouts::uniform(layout);

            assert_eq!(
                laid_out(MANIFEST, Some("docs"), Placement::Beside, &layouts).plans[0].path,
                PathBuf::from(".github/actions/pre-commit/README.md")
            );
            assert_eq!(
                laid_out(WORKFLOW, Some("docs"), Placement::Beside, &layouts).plans[0].path,
                PathBuf::from(".github/workflows/lint.md")
            );
        }
    }

    #[test]
    fn the_shapes_the_mirror_did_not_take_are_named() {
        let target = laid_out(
            MANIFEST,
            Some("docs"),
            Placement::Beside,
            &Layouts::uniform(Layout::DirectoryIndex),
        );

        assert_eq!(
            orphans(&target, Reason::Layout),
            [
                PathBuf::from("docs/actions/pre-commit.md"),
                PathBuf::from("docs/actions/pre-commit/README.md"),
            ]
        );
    }

    #[test]
    fn the_document_actually_written_is_never_called_stranded() {
        for layout in [Layout::Flat, Layout::Directory, Layout::DirectoryIndex] {
            let target = laid_out(
                MANIFEST,
                Some("docs"),
                Placement::Beside,
                &Layouts::uniform(layout),
            );
            let written = &target.plans[1].path;

            assert!(
                !target.orphans.iter().any(|orphan| orphan.path == *written),
                "{written:?} was reported as stranded"
            );
        }
    }

    #[test]
    fn nothing_is_stranded_where_nothing_is_mirrored() {
        assert!(action(None).orphans.is_empty());
    }

    #[test]
    fn a_rule_about_one_target_beats_a_rule_about_its_kind() {
        let layouts = Layouts {
            default: Layout::Flat,
            actions: Some(Layout::Directory),
            action: BTreeMap::from([("pre-commit".to_owned(), Layout::DirectoryIndex)]),
            ..Layouts::default()
        };

        assert_eq!(
            layouts.resolve(Kind::Action, "pre-commit"),
            Layout::DirectoryIndex
        );
        assert_eq!(layouts.resolve(Kind::Action, "other"), Layout::Directory);
        // Workflows were never mentioned, so they fall all the way through.
        assert_eq!(layouts.resolve(Kind::Workflow, "pre-commit"), Layout::Flat);
    }

    #[test]
    fn a_rule_names_one_kind_without_disturbing_the_other() {
        let layouts = Layouts {
            workflows: Some(Layout::DirectoryIndex),
            ..Layouts::default()
        };

        assert_eq!(
            laid_out(WORKFLOW, Some("docs"), Placement::Beside, &layouts).plans[1].path,
            PathBuf::from("docs/workflows/lint/index.md")
        );
        assert_eq!(
            laid_out(MANIFEST, Some("docs"), Placement::Beside, &layouts).plans[1].path,
            PathBuf::from("docs/actions/pre-commit.md")
        );
    }

    #[test]
    fn a_repository_that_states_one_shape_is_recognised_as_uniform() {
        assert!(Layouts::default().is_uniform());
        assert!(Layouts::uniform(Layout::DirectoryIndex).is_uniform());
        assert!(
            !Layouts {
                workflows: Some(Layout::Flat),
                ..Layouts::default()
            }
            .is_uniform()
        );
    }

    #[test]
    fn one_word_and_a_table_are_both_accepted() {
        #[derive(Deserialize)]
        struct Wrapper {
            layout: Layouts,
        }

        let word: Wrapper = toml::from_str("layout = \"directory-index\"\n").unwrap();
        assert_eq!(word.layout, Layouts::uniform(Layout::DirectoryIndex));

        let table: Wrapper =
            toml::from_str("[layout]\ndefault = \"flat\"\nworkflows = \"directory\"\n").unwrap();
        assert_eq!(table.layout.default, Layout::Flat);
        assert_eq!(table.layout.workflows, Some(Layout::Directory));
    }

    #[test]
    fn a_flat_layout_has_no_file_name_to_argue_about() {
        // The entry file is a property of having a directory, so it is spelled
        // into the layout rather than alongside it. There is no form of this
        // setting that pairs `flat` with a name, which is why nothing has to
        // validate against one.
        for rejected in [
            "{ entry = \"index\" }",
            "{ default = \"flat\", entry = \"index\" }",
            "\"flat-index\"",
            "\"flat-readme\"",
        ] {
            let document = format!("layout = {rejected}\n");
            let parsed: Result<BTreeMap<String, Layouts>, _> = toml::from_str(&document);

            assert!(parsed.is_err(), "accepted {rejected}");
        }
    }

    #[test]
    fn a_named_target_is_spelled_the_way_the_table_nests_it() {
        #[derive(Deserialize)]
        struct Wrapper {
            layout: Layouts,
        }

        let parsed: Wrapper = toml::from_str(
            "[layout]\ndefault = \"flat\"\n\n[layout.action]\npre-commit = \"directory\"\n",
        )
        .unwrap();

        assert_eq!(
            parsed.layout.resolve(Kind::Action, "pre-commit"),
            Layout::Directory
        );
        assert_eq!(parsed.layout.resolve(Kind::Action, "other"), Layout::Flat);
    }
}
