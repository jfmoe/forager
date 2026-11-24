# forager

A search and retrieval CLI for AI agents. forager organises search, web page reading, site mapping, documentation lookup and platform retrieval by capability, and handles provider selection, credential rotation and same-capability fallback in one place.

Output is JSON by default, for agents, scripts and CI; Markdown or plain content formats are available when a person reads the result.

## Installation

forager ships prebuilt binaries through [GitHub Releases](https://github.com/jfmoe/forager/releases); no local Rust installation is needed. A release provides:

- macOS: Apple Silicon, Intel
- Linux: AArch64, x86-64
- Windows: x86-64
- a SHA-256 checksum file for each platform archive
- shell and PowerShell installers
- build provenance attestations and `release-gate.json`

### macOS / Linux

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/jfmoe/forager/releases/latest/download/forager-installer.sh | sh
```

The installer puts the binary in `$CARGO_HOME/bin` or `$HOME/.cargo/bin` and configures `PATH` when needed. To upgrade, run the installer again.

### Windows PowerShell

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/jfmoe/forager/releases/latest/download/forager-installer.ps1 | iex"
```

To pin a version or inspect what gets installed first, download the installer or platform archive from the [Releases page](https://github.com/jfmoe/forager/releases) and verify the archive with the `.sha256` file of the same name.

## Setup

Create the configuration interactively:

```sh
forager setup
```

Check the configuration and provider status:

```sh
forager doctor --format markdown
```

The configuration is written to `~/.config/forager/config.toml` by default and respects `XDG_CONFIG_HOME`. View and change it with:

```sh
forager config path
forager config list
forager config set providers.exa.keys '["YOUR_API_KEY"]'
```

## Basic usage

Ordinary search:

```sh
forager search "latest progress on Rust async drop" --format markdown
```

Declare the capabilities the caller needs:

```sh
forager search "what changed in the OpenAI API recently" \
  --capabilities docs_search,web_search \
  --format markdown
```

Read a known web page:

```sh
forager fetch https://doc.rust-lang.org/cargo/ --format markdown
```

`fetch` reads URLs and PDFs through the configured Web Fetch chain, and on success returns only provider-independent Markdown; diagnostics and provider attempts are kept out of the body.

See every command:

```sh
forager --help
```

## Installing the Agent Skill

The CLI does the retrieval; the repository's `forager` skill teaches an agent to choose commands, declare capabilities and run a multi-source investigation from `search`, `fetch` and the platform commands. After installing the binary and configuring it, install the skill for the current project:

```sh
npx skills add jfmoe/forager
```

When the skill names a command or option the installed forager lacks, upgrade forager.

## Design boundaries

- `search` accepts the full capability set the caller declares; forager does not widen it on its own.
- The `sources` of `search` are only the main answer's sources; `extra_sources` carries every non-main Search Candidate produced by Supplemental Web Search, Documentation Search and search-side Web Fetch. Candidates keep the provider's own snippet; only search-side Web Fetch candidates carry a short preview of the page body.
- forager does not orchestrate investigations: the calling agent splits the question, chooses the commands and judges the evidence.
- `fetch`, `map` and the `exa` subcommands are explicit direct entry points.
- Credentials are kept in the local configuration; command output and the persisted journal redact sensitive URL parameters.
- When a capability is temporarily unavailable, the result reports it in `capability_gaps` instead of passing the missing capability off as covered.

Domain terms are in [GLOSSARY.md](GLOSSARY.md), the architecture in [docs/architecture.md](docs/architecture.md).

## Building from source

Building from source is for contributors, not the main way to install. The project pins its development toolchain in `rust-toolchain.toml`:

```sh
git clone https://github.com/jfmoe/forager.git
cd forager
git config core.hooksPath .githooks  # check formatting before each commit
cargo build --locked
```

Run the build as `./target/debug/forager`. `cargo install --path .` refuses to overwrite a forager that the installer placed, since cargo does not manage it; update an installed forager with the installer.

The checks to run before committing are listed in [AGENTS.md](AGENTS.md).

## Releases

release-plz manages the version and the changelog; dist builds the cross-platform archives, installers, checksums and the GitHub Release. A release is published only after the archive checks, architecture checks and checks that run each packaged binary pass for all five target platforms.
