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

/// What breaks the rule in one store directory, relative to `tests/samples`: `directory` is its
/// name, `entries` the names in it, and `manifest` the manifest's text when it has one.
pub(crate) fn violations(
    directory: &str,
    entries: &[String],
    manifest: Option<&str>,
) -> Vec<String> {
    let samples = entries
        .iter()
        .filter(|entry| entry.as_str() != MANIFEST)
        .collect::<BTreeSet<_>>();
    let Some(manifest) = manifest else {
        return vec![format!("{directory}: no {MANIFEST} records its samples")];
    };
    let manifest = match toml::from_str::<Manifest>(manifest) {
        Ok(manifest) => manifest,
        Err(error) => return vec![format!("{directory}/{MANIFEST}: {}", error.message())],
    };
    let recorded = manifest
        .samples
        .iter()
        .map(|record| &record.file)
        .collect::<BTreeSet<_>>();
    let mut found = Vec::new();
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
        match (record.origin, &record.obtained) {
            (Origin::Captured, None) => {
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
