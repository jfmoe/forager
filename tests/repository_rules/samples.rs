//! The sample store: `tests/samples/<provider>/` holds a provider's upstream samples beside one
//! manifest, `samples.toml`, with a metadata record for each sample. A sample without a record
//! leaves a reader unable to tell how far to trust it, and a record without a sample is stale.

use std::collections::BTreeSet;

use serde::Deserialize;

pub(crate) const MANIFEST: &str = "samples.toml";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(rename = "sample", default)]
    samples: Vec<Record>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    file: String,
    shape: String,
    variant_of: Option<String>,
    origin: Origin,
    observed: toml::value::Datetime,
    obtained: Option<String>,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Origin {
    Captured,
    HandWritten,
}

/// One name in a store directory.
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) is_directory: bool,
}

/// What breaks the rule in one store directory, relative to `tests/samples`: `directory` is its
/// name, `entries` what it holds, and `manifest` the manifest's text when it has one.
pub(crate) fn violations(
    directory: &str,
    entries: &[Entry],
    manifest: Option<&str>,
) -> Vec<String> {
    // A nested directory would hide its files from the record check, and a provider's samples
    // are few enough to sit side by side.
    let mut found = entries
        .iter()
        .filter(|entry| entry.is_directory)
        .map(|entry| {
            format!(
                "{directory}/{}: a provider's samples sit directly in its directory",
                entry.name
            )
        })
        .collect::<Vec<_>>();
    let samples = entries
        .iter()
        .filter(|entry| !entry.is_directory && entry.name != MANIFEST)
        .map(|entry| &entry.name)
        .collect::<BTreeSet<_>>();
    let Some(manifest) = manifest else {
        found.push(format!("{directory}: no {MANIFEST} records its samples"));
        return found;
    };
    let manifest = match toml::from_str::<Manifest>(manifest) {
        Ok(manifest) => manifest,
        Err(error) => {
            found.push(format!("{directory}/{MANIFEST}: {}", error.message()));
            return found;
        }
    };
    let recorded = manifest
        .samples
        .iter()
        .map(|record| &record.file)
        .collect::<BTreeSet<_>>();
    for sample in &samples {
        if !recorded.contains(sample) {
            found.push(format!(
                "{directory}/{sample}: no metadata record in {MANIFEST}"
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for record in &manifest.samples {
        let name = format!("{directory}/{MANIFEST}: the record of `{}`", record.file);
        if !seen.insert(&record.file) {
            found.push(format!("{name} appears twice"));
        }
        if !samples.contains(&record.file) {
            found.push(format!("{name} names no sample in {directory}/"));
        }
        if record.shape.trim().is_empty() {
            found.push(format!(
                "{name} does not say which response structure it holds"
            ));
        }
        if record
            .variant_of
            .as_ref()
            .is_some_and(|base| !recorded.contains(base))
        {
            found.push(format!("{name} is a variant of a sample with no record"));
        }
        if record.observed.date.is_none() || record.observed.time.is_some() {
            found.push(format!(
                "{name} gives `observed` as something other than a date"
            ));
        }
        match (record.origin, record.obtained.as_deref().map(str::trim)) {
            (Origin::Captured, None | Some("")) => {
                found.push(format!(
                    "{name} is captured and does not say how it was obtained"
                ));
            }
            (Origin::HandWritten, Some(_)) => {
                found.push(format!(
                    "{name} is hand-written and says how it was obtained"
                ));
            }
            _ => {}
        }
    }
    found
}
