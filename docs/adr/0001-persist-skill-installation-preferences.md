---
status: superseded
---

# Persist Skill installation preferences and synchronize on first use

This decision is retained as a record of the Python-era Skill subsystem. forager does not implement Skill installation preferences or automatic synchronization; installation is delegated to `npx skills add jfmoe/forager`.

Smart Search will persist the user's complete set of Skill Containers in the existing `config.json` and synchronize the bundled `smart-search-cli` Skill when a different CLI version is first used. This keeps Skill and CLI versions aligned across package managers without allowing npm `postinstall` to write user-selected directories, while making the configured paths—not tool detection or the most recent successful write—the source of truth.

## Decision

- `config.json` contains `skills.schema_version`, a homogeneous `skills.paths` array, and `skills.last_synced_cli_version`. Paths do not retain whether they came from a built-in name or custom input.
- The only built-in names are `agents`, `claude`, and `hermes`, resolving respectively to `~/.agents/skills`, `~/.claude/skills`, and `~/.hermes/skills`. Other destinations are supplied as custom Skill Containers; installation always appends `smart-search-cli`.
- `smart-search skills install [TARGET_OR_PATH ...]` installs and completely replaces the saved path set. With no arguments it selects only `agents`. Partial write failure still saves the complete requested set and returns a non-zero exit code.
- `smart-search skills status` and `smart-search skills update` operate only on saved paths. `smart-search skills clear` saves an empty set and disables automatic synchronization without deleting installed files.
- Existing Skill CLI parameters and target aliases are removed without a compatibility period. Interactive setup defaults to `agents`; non-interactive provider setup does not alter Skill preferences unless the dedicated Skill command is used.
- When no Skill configuration exists, first use initializes it to `agents`; legacy target directories are neither detected nor migrated. Any CLI version-string change, including downgrade or release-channel switch, makes synchronization pending.
- Automatic synchronization runs before ordinary commands but not help, version, or `skills` management commands. Success is silent. Failure warns on stderr, does not change the original command's result, and leaves the version pending for retry.
- Saved paths are expanded, made absolute, normalized and platform-deduplicated without resolving symlinks. Empty paths, filesystem roots, and existing non-directories are rejected; missing directories may be created.
- Cross-process synchronization is serialized. Managed file and configuration writes use same-directory temporary files and atomic replacement. Current managed files may be overwritten, but extra or obsolete files are reported and never automatically deleted.

## Consequences

The CLI gains a small amount of startup state management and locking, but package installation stays free of user-directory mutation. Users must express non-default destinations explicitly, old CLI flags fail immediately, and removing a configured target stops future synchronization without uninstalling its existing files.
