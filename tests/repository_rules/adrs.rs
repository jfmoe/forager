//! ADR files follow one naming and format rule, and every ADR reference names an existing ADR in
//! one form.

use std::collections::{BTreeMap, BTreeSet};

/// File names in `docs/adr/` that are not `NNNN-kebab-case-slug.md`, or that reuse a number.
pub fn file_name_problems<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut by_number = BTreeMap::<u16, Vec<&str>>::new();
    for name in names {
        match adr_number(name) {
            Some(number) => by_number.entry(number).or_default().push(name),
            None => problems.push(format!(
                "{name}: name an ADR with a four-digit number and a kebab-case slug, such as 0042-keep-one-chain.md"
            )),
        }
    }
    for (number, names) in by_number {
        if names.len() > 1 {
            problems.push(format!(
                "ADR-{number:04} is used by {names:?}: give each ADR its own number, the highest ever used plus one"
            ));
        }
    }
    problems
}

/// The number of an ADR file named `NNNN-kebab-case-slug.md`.
pub fn adr_number(name: &str) -> Option<u16> {
    let stem = name.strip_suffix(".md")?;
    let (number, slug) = stem.split_once('-')?;
    let is_slug = slug.split('-').all(|word| {
        !word.is_empty()
            && word
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    });
    (number.len() == 4 && number.chars().all(|c| c.is_ascii_digit()) && is_slug)
        .then(|| number.parse().ok())
        .flatten()
}

/// Why `markdown` is not an ADR in force: it has front matter or a status field.
pub fn format_problem(markdown: &str) -> Option<&'static str> {
    if markdown
        .lines()
        .next()
        .is_some_and(|line| line.trim() == "---")
    {
        return Some("remove the front matter; an ADR starts with its title");
    }
    let has_status = markdown.lines().any(|line| {
        let line = line.trim().to_ascii_lowercase();
        line.starts_with("status:") || line.trim_start_matches('#').trim() == "status"
    });
    has_status.then_some(
        "remove the status field; the ADR set holds only decisions in force, and a superseded ADR is deleted",
    )
}

/// ADR references in `text` that are not written `ADR-NNNN`, use list shorthand, or name an ADR
/// outside `existing`. With `in_adr`, references inside the `## Supersedes` section may name
/// deleted ADRs.
pub fn reference_problems(text: &str, existing: &BTreeSet<u16>, in_adr: bool) -> Vec<String> {
    let mut problems = Vec::new();
    let mut in_supersedes = false;
    for (index, line) in text.lines().enumerate() {
        if in_adr && line.starts_with('#') {
            in_supersedes = line.trim_start_matches('#').trim() == "Supersedes";
        }
        for reference in references(line) {
            let problem = match reference {
                Reference::Valid {
                    number,
                    list_follows,
                } => {
                    if list_follows {
                        Some(format!(
                            "ADR-{number:04} starts a list of numbers; write ADR-NNNN before every number"
                        ))
                    } else if !existing.contains(&number) && !in_supersedes {
                        Some(format!(
                            "ADR-{number:04} does not exist; point to the ADR that replaced it, or drop the reference"
                        ))
                    } else {
                        None
                    }
                }
                Reference::Malformed(written) => Some(format!(
                    "`{written}`: write an ADR reference as ADR-NNNN, one per number"
                )),
            };
            problems.extend(problem.map(|problem| format!("line {}: {problem}", index + 1)));
        }
    }
    problems
}

enum Reference {
    Valid { number: u16, list_follows: bool },
    Malformed(String),
}

fn references(line: &str) -> Vec<Reference> {
    line.match_indices("ADR")
        .filter(|(start, _)| {
            !line[..*start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
        .filter_map(|(start, _)| reference_at(&line[start + 3..], &line[start..]))
        .collect()
}

/// The reference that `rest`, the text after an `ADR` token, continues into.
fn reference_at(rest: &str, from: &str) -> Option<Reference> {
    if let Some(after_dash) = rest.strip_prefix('-') {
        let digits = leading_digits(after_dash);
        let after = &after_dash[digits.len()..];
        if digits.is_empty() {
            return None;
        }
        if digits.len() != 4 || after.starts_with(char::is_alphanumeric) {
            return Some(Reference::Malformed(written(from)));
        }
        return Some(Reference::Valid {
            number: digits.parse().ok()?,
            list_follows: list_follows(after),
        });
    }
    let rest = rest.strip_prefix('s').unwrap_or(rest);
    let after_space = rest.trim_start_matches([' ', '\u{a0}']);
    (after_space.len() < rest.len() && !leading_digits(after_space).is_empty())
        .then(|| Reference::Malformed(written(from)))
}

/// Whether `after`, the text after a reference, continues with a bare number as in `ADR-0018,
/// 0029` or `ADR-0018、0029`.
fn list_follows(after: &str) -> bool {
    let after = after.trim_start();
    [",", "、", "/", "&", "and ", "or ", "和", "与", "及", "–"]
        .iter()
        .find_map(|separator| after.strip_prefix(separator))
        .is_some_and(|next| leading_digits(next.trim_start()).len() == 4)
}

fn leading_digits(text: &str) -> &str {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    &text[..end]
}

/// The reference as written, up to the end of its number.
fn written(from: &str) -> String {
    let number_start = from
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(from.len());
    let number = leading_digits(&from[number_start..]);
    from[..number_start + number.len()].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing() -> BTreeSet<u16> {
        BTreeSet::from([7, 29])
    }

    #[test]
    fn reports_malformed_names_and_reused_numbers() {
        let problems = file_name_problems([
            "0007-primary-first.md",
            "0007-other.md",
            "29-short.md",
            "0030-Upper-Case.md",
            "0031_snake.md",
            "+123-plus.md",
            "README.md",
        ]);
        let reported = problems
            .iter()
            .map(|problem| problem.split(':').next().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(
            reported,
            [
                "29-short.md",
                "0030-Upper-Case.md",
                "0031_snake.md",
                "+123-plus.md",
                "README.md",
                "ADR-0007 is used by [\"0007-primary-first.md\", \"0007-other.md\"]",
            ]
        );
    }

    #[test]
    fn accepts_numbered_kebab_case_names() {
        assert_eq!(
            file_name_problems(["0007-keep-one-chain.md", "0029-fetch-via-2-providers.md"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn rejects_front_matter_and_status_fields() {
        assert!(format_problem("---\nstatus: accepted\n---\n\n# Title\n").is_some());
        assert!(format_problem("# Title\n\nStatus: superseded by ADR-0004\n").is_some());
        assert!(format_problem("# Title\n\n## Status\n\nAccepted\n").is_some());
        assert_eq!(
            format_problem("# Title\n\n## Decision\n\nThe status quo stays.\n"),
            None
        );
    }

    #[test]
    fn reports_other_forms_of_reference() {
        let problems = reference_problems(
            "See ADR 0007, ADR-07 and ADR-00070.\nADR 29 too, and ADRs 0007-0029.",
            &existing(),
            false,
        );
        assert_eq!(problems.len(), 5, "{problems:#?}");
        assert!(
            problems[0].starts_with("line 1: `ADR 0007`"),
            "{problems:#?}"
        );
        assert!(problems[3].starts_with("line 2: `ADR 29`"), "{problems:#?}");
    }

    #[test]
    fn reports_list_shorthand() {
        for text in [
            "ADR-0007、0029",
            "ADR-0007, 0029",
            "ADR-0007/0029",
            "ADR-0007 and 0029",
            "ADR-0007 or 0029",
            "ADR-0007 & 0029",
            "ADR-0007–0029",
            "ADR-0007和0029",
        ] {
            assert_eq!(
                reference_problems(text, &existing(), false).len(),
                1,
                "{text}"
            );
        }
    }

    #[test]
    fn reports_references_to_missing_adrs_outside_supersedes() {
        let adr = "# Title\n\nSee ADR-0012.\n\n## Supersedes\n\n- ADR-0012, merged here.\n";
        let problems = reference_problems(adr, &existing(), true);
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert!(problems[0].starts_with("line 3: ADR-0012 does not exist"));
        assert_eq!(
            reference_problems("- ADR-0012, merged here.", &existing(), false).len(),
            1
        );
    }

    #[test]
    fn accepts_existing_references_and_unrelated_words() {
        let text =
            "ADR-0007 and ADR-0029, ADR-0007/ADR-0029, ADRs, ADR-FORMAT.md, MADR 0007, (ADR-0029).";
        assert_eq!(
            reference_problems(text, &existing(), false),
            Vec::<String>::new()
        );
    }
}
