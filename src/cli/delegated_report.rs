//! Delivery of a completed Delegated Research Report to local files: one rule for every
//! `research result` command, whichever research agent wrote the report (ADR 0023).

use std::fs;
use std::path::{Path, PathBuf};

use super::dispatch::invocation_temp_dir;
use crate::types::{
    AttemptErrorKind, DelegatedReport, ProviderAttempt, ProviderError, ReportFiles,
};

/// How one research agent's reports are named on disk and in messages.
pub(super) struct ReportFiling {
    /// The research agent as messages name it, such as `Gemini`.
    pub(super) agent: &'static str,
    /// The file name prefix, such as `gemini` for `gemini-<id>.md`.
    pub(super) prefix: &'static str,
}

impl ReportFiling {
    /// Writes `<prefix>-<id>.md`, the body followed by its `## Sources` list, and
    /// `<prefix>-<id>.sources.json` into `report_dir`, or into a new directory under the system
    /// temporary directory. A write failure is Runtime, carries `attempts`, and never falls back
    /// to inline output.
    pub(super) fn write(
        &self,
        id: &str,
        report: &DelegatedReport,
        report_dir: Option<PathBuf>,
        attempts: &mut Vec<ProviderAttempt>,
        diagnostic: &mut Option<String>,
    ) -> Result<ReportFiles, ProviderError> {
        let directory =
            report_dir.unwrap_or_else(|| invocation_temp_dir(&format!("forager-{}", self.prefix)));
        self.write_files(report, &directory, &format!("{}-{id}", self.prefix))
            .map_err(|message| ProviderError {
                kind: AttemptErrorKind::Runtime,
                message,
                attempts: std::mem::take(attempts),
                verbose: false,
                diagnostic: diagnostic.take(),
                redirected_library_id: None,
            })
    }

    fn write_files(
        &self,
        report: &DelegatedReport,
        directory: &Path,
        stem: &str,
    ) -> Result<ReportFiles, String> {
        let report_path = directory.join(format!("{stem}.md"));
        let sources_path = directory.join(format!("{stem}.sources.json"));
        let failed = |path: &Path, error: std::io::Error| {
            format!(
                "cannot write the {} report to {}: {error}",
                self.agent,
                path.display()
            )
        };
        fs::create_dir_all(directory).map_err(|error| failed(directory, error))?;
        fs::write(&report_path, report.markdown()).map_err(|error| failed(&report_path, error))?;
        let sources = serde_json::to_vec_pretty(&report.sources)
            .map_err(|error| failed(&sources_path, std::io::Error::other(error)))?;
        fs::write(&sources_path, sources).map_err(|error| failed(&sources_path, error))?;
        Ok(ReportFiles {
            report_path: report_path.display().to_string(),
            sources_path: sources_path.display().to_string(),
        })
    }
}
