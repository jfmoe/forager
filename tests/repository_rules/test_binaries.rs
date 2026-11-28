//! The integration test binaries. Each binary links the library and the parts of the shared test
//! environment it uses again, so every binary added outside the fixed set rebuilds after any
//! source change; a test that spawns the binary belongs in the command contract binary instead.

use std::collections::BTreeSet;

/// The integration test binaries, by name: the command contract tests, the rule binary a document
/// change runs alone, the release shape tests, and the two targets the Windows job names.
pub(crate) const FIXED_SET: [&str; 5] = [
    "command_contract",
    "config_permissions",
    "release_artifact_gate",
    "release_scaffolding",
    "repository_rules",
];

/// One entry directly under `tests/`.
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) is_directory: bool,
    /// For a directory, whether it holds a `main.rs`.
    pub(crate) has_main: bool,
}

/// How each entry and each `[[test]]` target breaks the fixed set, as Cargo discovers binaries:
/// a `tests/<name>.rs` file or a `tests/<name>/main.rs` directory, unless the manifest turns
/// discovery off, and each `[[test]]` table the manifest declares.
pub(crate) fn violations(entries: &[Entry], manifest: &toml::Table) -> Vec<String> {
    let mut found = Vec::new();
    let package = manifest.get("package").and_then(toml::Value::as_table);
    if package
        .and_then(|package| package.get("autotests"))
        .and_then(toml::Value::as_bool)
        == Some(false)
    {
        found.push(
            "Cargo.toml: `autotests = false` hides every test file the manifest does not list, so a new one would run nowhere"
                .to_owned(),
        );
    }
    let declared = manifest
        .get("test")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .map(|target| {
            target
                .get("name")
                .and_then(toml::Value::as_str)
                .unwrap_or("<unnamed>")
                .to_owned()
        });
    found.extend(declared.map(|name| {
        format!("Cargo.toml: the `[[test]]` target `{name}` adds a binary; let Cargo discover the fixed set")
    }));

    let discovered = entries
        .iter()
        .filter(|entry| !entry.name.starts_with('.'))
        .filter_map(|entry| {
            if entry.is_directory {
                entry
                    .has_main
                    .then(|| (entry.name.clone(), format!("tests/{}/main.rs", entry.name)))
            } else {
                let stem = entry.name.strip_suffix(".rs")?;
                Some((stem.to_owned(), format!("tests/{}", entry.name)))
            }
        })
        .collect::<Vec<_>>();
    let names = discovered
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<BTreeSet<_>>();
    found.extend(
        discovered
            .iter()
            .filter(|(name, _)| !FIXED_SET.contains(&name.as_str()))
            .map(|(_, path)| {
                format!("{path}: a test binary outside the fixed set; make it a module of the command contract binary")
            }),
    );
    found.extend(
        FIXED_SET
            .iter()
            .filter(|name| !names.contains(*name))
            .map(|name| format!("tests/: the fixed binary `{name}` is missing")),
    );
    found
}
