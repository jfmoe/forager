//! The product skill's file rules: how much each file may hold, and that every file is linked
//! from the entry. They look at files and links, never at wording.

use std::collections::{BTreeMap, BTreeSet};

/// One file of the skill, by its path from the skill's root with `/` between parts.
pub(crate) struct File {
    pub(crate) path: String,
    pub(crate) text: String,
}

/// The most characters the whole skill may hold.
const WHOLE_BUDGET: usize = 70_000;

/// The most characters a file may hold: the entry and each platform's own file carry more than
/// a branch, shared, diagnostics or feedback file.
fn budget(path: &str) -> usize {
    let platform_file = path
        .strip_prefix("references/platforms/")
        .is_some_and(|name| name != "shared.md");
    if path == "SKILL.md" || platform_file {
        6_000
    } else {
        4_000
    }
}

/// Each file over its budget, and the whole skill over its own, counted in characters.
pub(crate) fn budget_violations(files: &[File]) -> Vec<String> {
    let mut found = Vec::new();
    let mut whole = 0;
    for file in files {
        let characters = file.text.chars().count();
        whole += characters;
        let budget = budget(&file.path);
        if characters > budget {
            found.push(format!(
                "{}: {characters} characters, budget {budget}",
                file.path
            ));
        }
    }
    if whole > WHOLE_BUDGET {
        found.push(format!(
            "the whole skill: {whole} characters, budget {WHOLE_BUDGET}"
        ));
    }
    found
}

/// Each relative link that points to no file of the skill, and each file no chain of links from
/// `SKILL.md` reaches.
pub(crate) fn link_violations(files: &[File]) -> Vec<String> {
    let paths = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    let mut found = Vec::new();
    let mut links = BTreeMap::new();
    for file in files {
        let mut resolved = Vec::new();
        for (target, path) in crate::links::relative_links(&file.text) {
            match resolve(&file.path, &path).filter(|path| paths.contains(path.as_str())) {
                Some(path) => resolved.push(path),
                None => found.push(format!(
                    "{} -> {target}: no such file in the skill",
                    file.path
                )),
            }
        }
        links.insert(file.path.as_str(), resolved);
    }

    let mut reached = BTreeSet::from(["SKILL.md".to_owned()]);
    let mut pending = vec!["SKILL.md".to_owned()];
    while let Some(path) = pending.pop() {
        for next in links.get(path.as_str()).into_iter().flatten() {
            if reached.insert(next.clone()) {
                pending.push(next.clone());
            }
        }
    }
    found.extend(
        files
            .iter()
            .filter(|file| !reached.contains(&file.path))
            .map(|file| format!("{}: no link from SKILL.md reaches it", file.path)),
    );
    found
}

/// The skill path that `target`, written in the file at `from`, names, or `None` when it climbs
/// out of the skill.
fn resolve(from: &str, target: &str) -> Option<String> {
    let mut parts = from.split('/').collect::<Vec<_>>();
    parts.pop();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}
