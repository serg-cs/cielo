use std::{collections::BTreeMap, env, fmt::Write as _, fs::OpenOptions, io::Write as _};

use anyhow::{Context, Result};

use crate::aemet::{AemetProduct, EntryFailure};

/// Outcome of one product in a data build.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ProductStatus {
    Published(usize),
    Failed(String),
}

impl ProductStatus {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Published(count) => format!("ok ({count})"),
            Self::Failed(error) => format!("failed: {error}"),
        }
    }

    pub(crate) const fn is_ok(&self) -> bool {
        matches!(self, Self::Published(_))
    }
}

/// Example municipality IDs listed for each failure group.
const EXAMPLES_PER_GROUP: usize = 5;
const STEP_SUMMARY_ENV: &str = "GITHUB_STEP_SUMMARY";

/// Municipalities that failed in the same way.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct FailureGroup {
    pub(super) product: AemetProduct,
    pub(super) pattern: String,
    pub(super) count: usize,
    pub(super) examples: Vec<String>,
}

/// Outcome of one data build, for logs and the CI step summary.
#[derive(Debug, Default)]
pub(super) struct DataBuildReport {
    pub(super) products: Vec<(AemetProduct, ProductStatus)>,
    pub(super) source_entries: usize,
    pub(super) published: usize,
    pub(super) failed: usize,
    pub(super) groups: Vec<FailureGroup>,
}

/// Group failures by their error text, with IDs and numbers masked.
pub(super) fn group_failures(failures: &[EntryFailure]) -> Vec<FailureGroup> {
    let mut groups = BTreeMap::<(&'static str, String), FailureGroup>::new();
    for failure in failures {
        let pattern = failure_pattern(&failure.error);
        let group = groups
            .entry((failure.product.as_str(), pattern.clone()))
            .or_insert_with(|| FailureGroup {
                product: failure.product,
                pattern,
                count: 0,
                examples: Vec::new(),
            });
        group.count += 1;
        if group.examples.len() < EXAMPLES_PER_GROUP {
            group.examples.push(failure.id.clone());
        }
    }

    let mut groups = groups.into_values().collect::<Vec<_>>();
    groups.sort_by_key(|group| std::cmp::Reverse(group.count));
    groups
}

/// Replace every run of digits so failures for different municipalities match.
pub(super) fn failure_pattern(error: &anyhow::Error) -> String {
    let mut pattern = String::new();
    let mut in_digits = false;
    for character in format!("{error:#}").chars() {
        if character.is_ascii_digit() {
            if !in_digits {
                pattern.push('#');
            }
            in_digits = true;
        } else {
            pattern.push(character);
            in_digits = false;
        }
    }
    pattern
}

impl DataBuildReport {
    /// Plain-text summary for the build log.
    pub(super) fn render_text(&self) -> String {
        let mut text = format!(
            "AEMET data: {} municipalities in source, {} published, {} failed",
            self.source_entries, self.published, self.failed,
        );
        for (product, status) in &self.products {
            let _ = write!(text, "\n  {}: {}", product.as_str(), status.label());
        }
        for group in &self.groups {
            let _ = write!(
                text,
                "\n  {} x {} ({}), e.g. {}",
                group.count,
                group.pattern,
                group.product.as_str(),
                group.examples.join(", "),
            );
        }
        text
    }

    /// Markdown summary for the GitHub Actions run page.
    pub(super) fn render_markdown(&self) -> String {
        let mut markdown = String::from("## AEMET data build\n\n");
        let _ = writeln!(
            markdown,
            "| In source | Published | Failed |\n|---|---|---|\n| {} | {} | {} |",
            self.source_entries, self.published, self.failed,
        );
        if !self.products.is_empty() {
            markdown.push_str("\n| Product | Status |\n|---|---|\n");
            for (product, status) in &self.products {
                let _ = writeln!(
                    markdown,
                    "| {} | {} |",
                    product.as_str(),
                    status.label().replace('|', "\\|")
                );
            }
        }
        if !self.groups.is_empty() {
            markdown.push_str(
                "\n### Failures\n\n| Count | Product | Error | Examples |\n|---|---|---|---|\n",
            );
            for group in &self.groups {
                let _ = writeln!(
                    markdown,
                    "| {} | {} | `{}` | {} |",
                    group.count,
                    group.product.as_str(),
                    group.pattern.replace('|', "\\|").replace('`', "'"),
                    group.examples.join(", "),
                );
            }
        }
        markdown
    }

    /// Append the Markdown summary to `GITHUB_STEP_SUMMARY` when running in Actions.
    pub(super) fn write_step_summary(&self) -> Result<()> {
        let Some(path) = env::var_os(STEP_SUMMARY_ENV) else {
            return Ok(());
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .context("failed to open GitHub step summary")?;
        file.write_all(self.render_markdown().as_bytes())
            .context("failed to write GitHub step summary")
    }
}
