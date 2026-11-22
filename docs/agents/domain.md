# Domain Docs

How the engineering skills consume this repository's domain documentation. The repository has a single context.

## Before exploring, read these

- **`GLOSSARY.md`** at the repository root.
- **`docs/adr/`**: the ADRs that touch the area you are about to work in.

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `GLOSSARY.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal: either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, name it as `ADR-NNNN` and say why it is worth reopening, rather than silently overriding it.

## ADR format in this repository

This repository's ADRs differ from the `/domain-modeling` default; follow these rules instead of its template:

- Every ADR has the same sections: a title, then `Decision`, `Why` (with the key evidence figures), `Rejected alternatives`, `Revisit when`, and `Supersedes` when it replaces deleted ADRs, giving their numbers and the reason.
- There is no front matter and no status field. The set holds only decisions in force: a superseded ADR is deleted and named in its successor's `Supersedes` section, not marked as superseded.
- A new ADR takes the highest number ever used plus one, not the highest number now present: numbers of deleted ADRs are never reused, because commit history, the changelog and old issues cite them. A merged ADR keeps the newest number it inherits.
- An ADR states no values, key names or field names the code states, and does not point to another document for details.
