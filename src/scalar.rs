//! A single optional value from a source document, and the rules for rendering
//! one as Markdown.
//!
//! Action and workflow metadata is full of optional scalars: an input may
//! declare a `default`, or not; a `description`, or not. The difference between
//! *absent* and *present but empty* is load-bearing — an absent default renders
//! as `n/a` while an empty one renders as an empty code span — so the two are
//! kept apart all the way to the renderer instead of being flattened to `""`
//! during parsing.

use serde::{Serialize, Serializer};

/// Stand-in for an absent value inside a table cell.
const NA_CELL: &str = "n/a";
/// Stand-in for an absent value used as a section body.
const NA_SECTION: &str = "N/A";
const YES: &str = "yes";
const NO: &str = "no";
const PRE_OPEN: &str = "<pre>";
const PRE_CLOSE: &str = "</pre>";
const LINE_BREAK: &str = "<br>";
/// GitHub-flavoured Markdown splits table rows on `|` before it parses inline
/// content, so a literal pipe has to be escaped even inside a code span or a
/// `<pre>` block. Leaving it unescaped silently corrupts the whole table.
const ESCAPED_PIPE: &str = "\\|";

/// An optional string, plus the Markdown rendering rules that depend on it.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scalar(Option<String>);

impl Scalar {
    /// A value that was absent from the source document.
    pub const fn null() -> Self {
        Self(None)
    }

    /// A value that was present.
    pub fn new(value: impl Into<String>) -> Self {
        Self(Some(value.into()))
    }

    /// Whether the key appeared in the source document at all.
    pub fn is_present(&self) -> bool {
        self.0.is_some()
    }

    /// Whether the value is exactly `true`.
    ///
    /// GitHub only honours a literal `true`, so `yes`, `True` and `1` are all
    /// deliberately not required. Matching loosely here would document an input
    /// as required that Actions would happily accept without.
    pub fn is_true(&self) -> bool {
        self.0.as_deref() == Some("true")
    }

    /// The underlying value, if present.
    pub fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// A free-text table cell, such as a description. Absent renders empty,
    /// which reads better in a table than a placeholder would.
    pub fn cell_text(&self) -> String {
        match &self.0 {
            None => String::new(),
            Some(value) if value.contains('\n') => block(value),
            Some(value) => escape_pipes(value),
        }
    }

    /// A code-span table cell, such as `default` or `type`. Absent renders
    /// `n/a`, distinguishing "no default" from "defaults to the empty string".
    pub fn cell_code(&self) -> String {
        match &self.0 {
            None => NA_CELL.to_owned(),
            Some(value) if value.contains('\n') => block(value),
            Some(value) => code_span(&escape_pipes(value)),
        }
    }

    /// A boolean table cell, such as `required`.
    pub fn cell_flag(&self) -> &'static str {
        if self.is_true() { YES } else { NO }
    }

    /// A one-line table cell, such as an index summary.
    ///
    /// An index row has to stay on one line whatever the source did, so runs of
    /// whitespace — including the line breaks of a folded YAML scalar — collapse
    /// to single spaces. A `<pre>` block would be correct but unreadable in a
    /// table whose whole job is to be skimmed.
    pub fn cell_summary(&self) -> String {
        match &self.0 {
            None => String::new(),
            Some(value) => escape_pipes(&collapse(value)),
        }
    }

    /// The body of a Markdown section rather than a table cell, so it is
    /// emitted verbatim: no pipe escaping and no `<pre>` wrapping, because
    /// nothing about the surrounding syntax constrains it.
    pub fn section_text(&self) -> String {
        match &self.0 {
            None => NA_SECTION.to_owned(),
            Some(value) => value.trim().to_owned(),
        }
    }
}

impl From<String> for Scalar {
    fn from(value: String) -> Self {
        Self(Some(value))
    }
}

impl From<&str> for Scalar {
    fn from(value: &str) -> Self {
        Self(Some(value.to_owned()))
    }
}

impl From<Option<String>> for Scalar {
    fn from(value: Option<String>) -> Self {
        Self(value)
    }
}

impl Serialize for Scalar {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.0 {
            Some(value) => serializer.serialize_str(value),
            None => serializer.serialize_none(),
        }
    }
}

/// Escape pipes so a value cannot break out of its table cell.
pub(crate) fn escape_pipes(value: &str) -> String {
    if value.contains('|') {
        value.replace('|', ESCAPED_PIPE)
    } else {
        value.to_owned()
    }
}

/// Collapse every run of whitespace into a single space, and trim the ends.
///
/// `split_whitespace` already treats a run of any whitespace as one separator,
/// which is the whole rule, and it avoids taking a regex dependency for it.
fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Fold a multi-line value into something that survives a single table row.
///
/// A table cell cannot contain a real line break, so the value becomes a
/// `<pre>` block with `<br>` separators. Exactly one trailing newline is
/// dropped, because that is what a clipped YAML block scalar leaves behind and
/// it is never meaningful; any further blank lines are preserved.
fn block(value: &str) -> String {
    let body = value.strip_suffix('\n').unwrap_or(value);
    let mut out = String::with_capacity(body.len() + PRE_OPEN.len() + PRE_CLOSE.len());
    out.push_str(PRE_OPEN);
    for character in body.chars() {
        match character {
            '\n' => out.push_str(LINE_BREAK),
            // Carriage returns would otherwise survive into the output of a
            // CRLF source file and show up as stray whitespace.
            '\r' => {}
            '|' => out.push_str(ESCAPED_PIPE),
            other => out.push(other),
        }
    }
    out.push_str(PRE_CLOSE);
    out
}

/// Wrap a value in a code span whose fence is long enough to contain it.
///
/// `CommonMark` ends a code span at the first backtick run of matching length, so
/// a value containing backticks needs a longer fence, and one that starts or
/// ends with a backtick needs padding spaces that the renderer then strips.
fn code_span(value: &str) -> String {
    let longest_run = value
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest_run + 1);

    // An all-space value would otherwise have its content stripped entirely.
    let needs_padding = value.starts_with('`')
        || value.ends_with('`')
        || (!value.is_empty() && value.chars().all(|character| character == ' '));

    if needs_padding {
        format!("{fence} {value} {fence}")
    } else {
        format!("{fence}{value}{fence}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_and_empty_are_different_values() {
        assert!(!Scalar::null().is_present());
        assert!(Scalar::new("").is_present());
    }

    #[test]
    fn only_a_literal_true_counts_as_true() {
        assert!(Scalar::new("true").is_true());
        assert!(!Scalar::new("True").is_true());
        assert!(!Scalar::new("yes").is_true());
        assert!(!Scalar::new("1").is_true());
        assert!(!Scalar::new("false").is_true());
        assert!(!Scalar::null().is_true());
    }

    #[test]
    fn text_cell_renders_absent_as_empty() {
        assert_eq!(Scalar::null().cell_text(), "");
    }

    #[test]
    fn text_cell_passes_single_lines_through() {
        assert_eq!(
            Scalar::new("The pull-request title to validate.").cell_text(),
            "The pull-request title to validate."
        );
    }

    #[test]
    fn text_cell_keeps_inline_markdown() {
        assert_eq!(
            Scalar::new("use `skip` instead").cell_text(),
            "use `skip` instead"
        );
    }

    #[test]
    fn text_cell_escapes_pipes() {
        assert_eq!(Scalar::new("a | b").cell_text(), "a \\| b");
    }

    #[test]
    fn text_cell_folds_multiple_lines() {
        assert_eq!(
            Scalar::new("one\ntwo\nthree").cell_text(),
            "<pre>one<br>two<br>three</pre>"
        );
    }

    #[test]
    fn text_cell_escapes_pipes_inside_a_block() {
        assert_eq!(
            Scalar::new("a | b\nc").cell_text(),
            "<pre>a \\| b<br>c</pre>"
        );
    }

    #[test]
    fn block_drops_exactly_one_trailing_newline() {
        assert_eq!(
            Scalar::new("one\ntwo\n").cell_text(),
            "<pre>one<br>two</pre>"
        );
        assert_eq!(Scalar::new("one\n\n").cell_text(), "<pre>one<br></pre>");
    }

    #[test]
    fn block_drops_carriage_returns() {
        assert_eq!(
            Scalar::new("one\r\ntwo").cell_text(),
            "<pre>one<br>two</pre>"
        );
    }

    #[test]
    fn a_lone_carriage_return_is_not_a_line_break() {
        assert_eq!(Scalar::new("one\rtwo").cell_text(), "one\rtwo");
    }

    #[test]
    fn code_cell_renders_absent_as_na() {
        assert_eq!(Scalar::null().cell_code(), "n/a");
    }

    #[test]
    fn code_cell_wraps_values_in_backticks() {
        assert_eq!(Scalar::new("72").cell_code(), "`72`");
        assert_eq!(Scalar::new(".").cell_code(), "`.`");
        assert_eq!(Scalar::new("feat,fix,docs").cell_code(), "`feat,fix,docs`");
    }

    #[test]
    fn code_cell_distinguishes_empty_from_absent() {
        assert_eq!(Scalar::new("").cell_code(), "``");
        assert_eq!(Scalar::null().cell_code(), "n/a");
    }

    #[test]
    fn code_cell_widens_the_fence_around_backticks() {
        assert_eq!(Scalar::new("a`b").cell_code(), "``a`b``");
        assert_eq!(Scalar::new("a``b").cell_code(), "```a``b```");
    }

    #[test]
    fn code_cell_pads_values_that_touch_a_backtick() {
        assert_eq!(Scalar::new("`x").cell_code(), "`` `x ``");
        assert_eq!(Scalar::new("x`").cell_code(), "`` x` ``");
    }

    #[test]
    fn code_cell_pads_an_all_space_value() {
        assert_eq!(Scalar::new(" ").cell_code(), "`   `");
    }

    #[test]
    fn code_cell_escapes_pipes() {
        assert_eq!(Scalar::new("a|b").cell_code(), "`a\\|b`");
    }

    #[test]
    fn code_cell_uses_a_block_for_multiple_lines() {
        assert_eq!(
            Scalar::new("{\n  \"key\": \"value\"\n}").cell_code(),
            "<pre>{<br>  \"key\": \"value\"<br>}</pre>"
        );
    }

    #[test]
    fn flag_cell_is_yes_only_for_true() {
        assert_eq!(Scalar::new("true").cell_flag(), "yes");
        assert_eq!(Scalar::new("false").cell_flag(), "no");
        assert_eq!(Scalar::new("yes").cell_flag(), "no");
        assert_eq!(Scalar::null().cell_flag(), "no");
    }

    #[test]
    fn section_renders_absent_as_na() {
        assert_eq!(Scalar::null().section_text(), "N/A");
    }

    #[test]
    fn section_trims_surrounding_whitespace() {
        assert_eq!(
            Scalar::new("\n  Runs hooks.  \n").section_text(),
            "Runs hooks."
        );
    }

    #[test]
    fn section_keeps_line_breaks_and_pipes_verbatim() {
        assert_eq!(Scalar::new("one\ntwo").section_text(), "one\ntwo");
        assert_eq!(Scalar::new("a | b").section_text(), "a | b");
    }

    #[test]
    fn serialises_as_a_string_or_null() {
        assert_eq!(serde_json::to_string(&Scalar::new("x")).unwrap(), "\"x\"");
        assert_eq!(serde_json::to_string(&Scalar::new("")).unwrap(), "\"\"");
        assert_eq!(serde_json::to_string(&Scalar::null()).unwrap(), "null");
    }

    #[test]
    fn converts_from_common_sources() {
        assert_eq!(Scalar::from("x"), Scalar::new("x"));
        assert_eq!(Scalar::from("x".to_owned()), Scalar::new("x"));
        assert_eq!(Scalar::from(Some("x".to_owned())), Scalar::new("x"));
        assert_eq!(Scalar::from(None::<String>), Scalar::null());
    }

    #[test]
    fn summary_cell_renders_absent_as_empty() {
        assert_eq!(Scalar::null().cell_summary(), "");
    }

    #[test]
    fn summary_cell_collapses_every_run_of_whitespace() {
        assert_eq!(
            Scalar::new("  Runs\n  hooks,\n\n  eventually.  ").cell_summary(),
            "Runs hooks, eventually."
        );
    }

    #[test]
    fn summary_cell_never_becomes_a_block() {
        assert_eq!(Scalar::new("one\ntwo").cell_summary(), "one two");
    }

    #[test]
    fn summary_cell_escapes_pipes() {
        assert_eq!(Scalar::new("a | b").cell_summary(), "a \\| b");
    }
}
