//! Layered settings, resolved in one direction.
//!
//! A flag beats an environment variable, which beats a file, which beats a
//! built-in default. The ordering follows how deliberately and how narrowly a
//! value was stated: a flag is someone typing now, a file is a decision made
//! months ago by someone else, and the more immediate one should win.
//!
//! Clap collapses the first two for us. An argument with `env` and no
//! `default_value` is `Some` when either the flag or the variable supplied it,
//! and `None` when neither did — which is exactly the gap a file fills.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::render::usage::Pin;
use crate::sync::Options;
use crate::target::Placement;

/// Where a configuration file is looked for, in the order it is preferred.
///
/// The first that exists is the one used, outright. Merging several
/// repository-level files would put the effective settings in no single place
/// a reader could open, so the others are named on stderr and ignored.
pub const FILES: [&str; 4] = [
    ".actdocs.toml",
    "actdocs.toml",
    "config/actdocs.toml",
    ".config/actdocs.toml",
];

/// Deliberate placeholders. A generator that reached into the local clone for
/// these would produce documentation that differed between a fork, a working
/// copy and CI, so the caller states them or gets something obviously unset.
const REPO_SLUG: &str = "<owner>/<repo>";
const REF_SHA: &str = "<sha>";
const REF_VERSION: &str = "<version>";

/// One layer of settings.
///
/// Every field is optional because a layer says only what it wants to override.
/// `None` means "defer to the layer below", never "off".
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
// Keys match the flags they mirror, minus the dashes, so that knowing one form
// means knowing the other. An unrecognised key is rejected rather than
// ignored: a misspelling that silently does nothing is the worst outcome
// available, and this tool reports rather than guesses everywhere else.
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    pub docs_dir_target: Option<PathBuf>,
    pub index_target: Option<PathBuf>,
    pub workflow_docs: Option<Placement>,
    pub repo_slug: Option<String>,
    pub ref_sha: Option<String>,
    pub ref_version: Option<String>,
    pub pin: Option<Pin>,
}

impl Config {
    /// Read a layer from disk.
    ///
    /// A file named explicitly must exist, and replaces the search rather than
    /// joining it: naming a file and having it quietly ignored costs an
    /// afternoon. The files looked for by default need not exist, because most
    /// repositories will not have one.
    pub fn load(root: &Path, named: Option<&Path>, log: &mut dyn io::Write) -> Result<Self> {
        if let Some(named) = named {
            let path = root.join(named);
            let text = fs::read_to_string(&path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            return parse(&text, &path);
        }

        let existing: Vec<&str> = FILES
            .into_iter()
            .filter(|name| root.join(name).exists())
            .collect();

        let Some((&chosen, shadowed)) = existing.split_first() else {
            return Ok(Self::default());
        };

        // Naming both sides: knowing a file was ignored is only half of what
        // someone needs to fix it.
        for name in shadowed {
            writeln!(log, "{name}: ignored, because {chosen} takes precedence")?;
        }

        let path = root.join(chosen);
        let text =
            fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
        parse(&text, &path)
    }

    /// Fill this layer's gaps from `fallback`. `self` always wins.
    ///
    /// Deliberately named after `Option::or`, because that is precisely what it
    /// does field by field, and because `cli.or(file)` reads in the direction
    /// precedence actually runs.
    #[must_use]
    pub fn or(self, fallback: Self) -> Self {
        Self {
            docs_dir_target: self.docs_dir_target.or(fallback.docs_dir_target),
            index_target: self.index_target.or(fallback.index_target),
            workflow_docs: self.workflow_docs.or(fallback.workflow_docs),
            repo_slug: self.repo_slug.or(fallback.repo_slug),
            ref_sha: self.ref_sha.or(fallback.ref_sha),
            ref_version: self.ref_version.or(fallback.ref_version),
            pin: self.pin.or(fallback.pin),
        }
    }

    /// Settle every remaining gap and become a runnable set of options.
    ///
    /// The two documentation targets have no default: `None` there is a
    /// decision — write nothing — rather than an unanswered question.
    ///
    /// Fails on the one combination that cannot mean anything. Moving workflow
    /// documents into a documentation root nobody named would write them
    /// nowhere at all, and a run that silently generates nothing costs more to
    /// diagnose than one that refuses to start.
    pub fn into_options(self, root: PathBuf, check: bool) -> Result<Options> {
        let workflow_docs = self.workflow_docs.unwrap_or_default();
        if workflow_docs == Placement::DocsDir && self.docs_dir_target.is_none() {
            bail!(
                "workflow-docs is docs-dir, but no docs-dir-target was given, \
                 so workflow documents would be written nowhere"
            );
        }

        Ok(Options {
            root,
            docs_dir: self.docs_dir_target,
            index: self.index_target,
            workflow_docs,
            check,
            repo_slug: self.repo_slug.unwrap_or_else(|| REPO_SLUG.to_owned()),
            ref_sha: self.ref_sha.unwrap_or_else(|| REF_SHA.to_owned()),
            ref_version: self.ref_version.unwrap_or_else(|| REF_VERSION.to_owned()),
            pin: self.pin.unwrap_or_default(),
        })
    }
}

fn parse(text: &str, path: &Path) -> Result<Config> {
    toml::from_str(text).with_context(|| format!("cannot parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (name, body) in files {
            let path = root.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        root
    }

    fn loaded(root: &Path) -> (Config, String) {
        let mut log = Vec::new();
        let config = Config::load(root, None, &mut log).unwrap();
        (config, String::from_utf8(log).unwrap())
    }

    fn slug(value: &str) -> Config {
        Config {
            repo_slug: Some(value.to_owned()),
            ..Config::default()
        }
    }

    #[test]
    fn a_file_supplies_what_the_flags_left_out() {
        let root = repository(&[(
            ".actdocs.toml",
            "repo-slug = \"acme/tools\"\npin = \"version\"\n",
        )]);

        let resolved = Config::default().or(loaded(root.path()).0);

        assert_eq!(resolved.repo_slug.as_deref(), Some("acme/tools"));
        assert_eq!(resolved.pin, Some(Pin::Version));
    }

    #[test]
    fn a_flag_beats_the_file_where_they_overlap() {
        let root = repository(&[(".actdocs.toml", "repo-slug = \"from/file\"\n")]);

        let resolved = slug("from/flag").or(loaded(root.path()).0);

        assert_eq!(resolved.repo_slug.as_deref(), Some("from/flag"));
    }

    #[test]
    fn layering_is_one_directional() {
        let (flag, file) = (slug("from/flag"), slug("from/file"));

        assert_ne!(flag.clone().or(file.clone()), file.or(flag));
    }

    #[test]
    fn a_default_settles_what_nothing_else_stated() {
        let options = Config::default()
            .into_options(PathBuf::from("."), false)
            .unwrap();

        assert_eq!(options.repo_slug, REPO_SLUG);
        assert_eq!(options.ref_sha, REF_SHA);
        assert_eq!(options.ref_version, REF_VERSION);
        assert_eq!(options.pin, Pin::Sha);
    }

    #[test]
    fn an_unstated_documentation_target_stays_unstated() {
        // No default here on purpose: absence means "write nothing", which is
        // an answer rather than a gap.
        let options = Config::default()
            .into_options(PathBuf::from("."), false)
            .unwrap();

        assert!(options.docs_dir.is_none());
        assert!(options.index.is_none());
    }

    #[test]
    fn every_documented_location_is_searched() {
        for name in FILES {
            let root = repository(&[(name, "repo-slug = \"acme/tools\"\n")]);

            assert_eq!(
                loaded(root.path()).0.repo_slug.as_deref(),
                Some("acme/tools"),
                "{name} was not found"
            );
        }
    }

    #[test]
    fn the_documented_order_decides_which_one_wins() {
        // Every pair rather than only adjacent ones, so that reordering the
        // list cannot quietly change which file a repository reads.
        for (rank, winner) in FILES.into_iter().enumerate() {
            for loser in FILES.into_iter().skip(rank + 1) {
                let root = repository(&[
                    (loser, "repo-slug = \"loser\"\n"),
                    (winner, "repo-slug = \"winner\"\n"),
                ]);

                assert_eq!(
                    loaded(root.path()).0.repo_slug.as_deref(),
                    Some("winner"),
                    "{loser} beat {winner}"
                );
            }
        }
    }

    #[test]
    fn a_shadowed_file_contributes_nothing() {
        let root = repository(&[
            (".actdocs.toml", "repo-slug = \"winner\"\n"),
            ("actdocs.toml", "pin = \"version\"\n"),
        ]);

        let (config, _) = loaded(root.path());

        assert_eq!(config.repo_slug.as_deref(), Some("winner"));
        assert!(config.pin.is_none(), "the files were merged");
    }

    #[test]
    fn a_shadowed_file_is_named_rather_than_dropped_in_silence() {
        let root = repository(&[
            (".actdocs.toml", "repo-slug = \"winner\"\n"),
            ("config/actdocs.toml", "repo-slug = \"loser\"\n"),
        ]);

        let (_, log) = loaded(root.path());

        assert!(log.contains("config/actdocs.toml"), "got {log}");
        assert!(log.contains(".actdocs.toml"), "got {log}");
    }

    #[test]
    fn a_misspelled_key_is_rejected_rather_than_ignored() {
        let root = repository(&[(".actdocs.toml", "repo_slug = \"acme/tools\"\n")]);
        let error = Config::load(root.path(), None, &mut Vec::new()).unwrap_err();

        assert!(
            format!("{error:#}").contains(".actdocs.toml"),
            "got {error:#}"
        );
    }

    #[test]
    fn a_named_file_replaces_the_search_rather_than_joining_it() {
        let root = repository(&[
            (".actdocs.toml", "repo-slug = \"discovered\"\n"),
            ("elsewhere.toml", "repo-slug = \"named\"\n"),
        ]);
        let mut buffer = Vec::new();

        let config =
            Config::load(root.path(), Some(Path::new("elsewhere.toml")), &mut buffer).unwrap();

        assert_eq!(config.repo_slug.as_deref(), Some("named"));

        // Nothing was shadowed, because nothing was searched for.
        let log = String::from_utf8(buffer).unwrap();
        assert!(log.is_empty(), "got {log}");
    }

    #[test]
    fn a_named_file_that_is_missing_is_an_error() {
        let root = tempfile::tempdir().unwrap();

        assert!(
            Config::load(
                root.path(),
                Some(Path::new("nowhere.toml")),
                &mut Vec::new()
            )
            .is_err()
        );
    }

    #[test]
    fn an_absent_file_is_neither_an_error_nor_a_remark() {
        let root = tempfile::tempdir().unwrap();
        let (config, log) = loaded(root.path());

        assert_eq!(config, Config::default());
        assert!(log.is_empty(), "got {log}");
    }

    #[test]
    fn the_pin_is_named_the_way_the_flag_names_it() {
        let root = repository(&[(".actdocs.toml", "pin = \"sha\"\n")]);

        assert_eq!(loaded(root.path()).0.pin, Some(Pin::Sha));
    }

    #[test]
    fn moving_workflow_documents_needs_somewhere_to_move_them() {
        let config = Config {
            workflow_docs: Some(Placement::DocsDir),
            ..Config::default()
        };

        let error = config.into_options(PathBuf::from("."), false).unwrap_err();

        assert!(
            format!("{error:#}").contains("docs-dir-target"),
            "got {error:#}"
        );
    }

    #[test]
    fn moving_them_is_allowed_once_there_is_a_root() {
        let config = Config {
            workflow_docs: Some(Placement::DocsDir),
            docs_dir_target: Some(PathBuf::from("docs")),
            ..Config::default()
        };

        let options = config.into_options(PathBuf::from("."), false).unwrap();

        assert_eq!(options.workflow_docs, Placement::DocsDir);
    }

    #[test]
    fn the_placement_is_named_the_way_the_flag_names_it() {
        let root = repository(&[(".actdocs.toml", "workflow-docs = \"docs-dir\"\n")]);

        assert_eq!(
            loaded(root.path()).0.workflow_docs,
            Some(Placement::DocsDir)
        );
    }
}
