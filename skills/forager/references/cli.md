# forager CLI reference

This reference documents the public CLI for `forager >=0.3.0`. Load it under the conditions given
in `SKILL.md`: for exact command syntax, non-routine commands, or diagnosis and recovery details.
Routine `search` and `research` stay on their branch references. Treat
`forager <command> --help` as the final authority for argument parsing.

## Contents

- [Shared behavior](#shared-behavior)
- [Smart pipelines](#smart-pipelines)
- [Direct operations](#direct-operations)
- [Provider-direct commands](#provider-direct-commands)
- [Configuration and diagnostics](#configuration-and-diagnostics)
- [Exit codes](#exit-codes)

## Shared behavior

The public top-level commands are `search`, `research`, `fetch`, `map`, `exa`, `context7`,
`anysearch`, `config`, `setup`, `doctor`, and `smoke`. Use `-h` or `--help` on any command for
parser-generated help. Available aliases are:

| Command | Alias |
| --- | --- |
| `search` | `s` |
| `research` | `rs` |
| `fetch` | `f` |
| `context7` | `c7` |
| `anysearch` | `as` |
| `config list` | `config ls` |

Network commands that expose these options share the following behavior:

| Option | Behavior |
| --- | --- |
| `--timeout SECONDS` | Set a positive hard deadline for the whole command, including retries and fallback. |
| `--format FORMAT` | Select stdout rendering. The default is `json`. |
| `--output FILE` | Write the same rendered result to `FILE` and still emit it to stdout. |
| `--verbose` | Include full provider attempts inline. Without it, `search` and `research` retain full attempts in their journal; provider-direct commands do not create a result journal. |

After argument parsing selects `--format json`, `search`, `fetch`, and `research` return a single
parseable error object on stdout for configuration, stdin, or plan preflight failures. Clap parser
errors and panics keep their existing channels, as do failures in non-JSON formats.

`log.level=debug` enables an optional terminal projection with one bounded attempts summary;
`trace` adds safe fields for each attempt. This projection never changes stdout or replaces
`--verbose` and journal attempts.

`content` output is available only for `search`, `research`, `fetch`, and `context7 docs`.
All other commands with `--format` accept only `json` and `markdown`. `smoke` emits JSON and does
not expose `--format`.

## Smart pipelines

### `search`

```console
forager search QUERY [--capabilities CSV|none] [--model ID] [--extra-sources N]
                     [--fallback auto|off] [--timeout SECONDS]
                     [--format json|markdown|content] [--output FILE] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Search request passed to the main-search pipeline. | Required |
| `--capabilities CSV\|none` | Authoritative supplemental capability set. Use canonical comma-separated IDs, or `none` for main search only. Omit it to use classifier routing, with the configured default Web route when no classifier is available. | Omitted |
| `--model ID` | Override the configured main-search model for this invocation. | Configured model |
| `--extra-sources N` | Set a target in `0..=20`. At `0`, Web Search uses 3 while Documentation Search and Vertical Search use 1; an explicit `1..=20` is the exact target. | `0` |
| `--fallback MODE` | Use `auto` or `off` for provider/model fallback. | `search.fallback`, normally `auto` |
| `--timeout SECONDS` | Set the whole-pipeline deadline. | `180` |
| `--format FORMAT` | Use `json`, `markdown`, or `content`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--verbose` | Include full provider attempts inline. | Off |

Default search JSON keeps `answer`, Primary Search Sources in `sources`, every non-primary Search
Candidate in `extra_sources`, capability gaps, and the journal reference. Each candidate has
required `provider`, `capability`, and provider-specific `provider_data`, plus nullable `title`,
`url`, and `summary`. Provider-native summaries are not verified evidence. Candidates with an
HTTP(S) URL can enter Web Fetch; a Context7 candidate carries a typed library locator in
`provider_data` for Documentation Search or the Research Evidence Pipeline and has no fabricated
URL. Search-side Web Fetch successes use the actual fetch provider and a 300-character Normalized
Fetch Content preview as their candidate summary. `--verbose` adds provider attempts inline.
Content format emits only the main answer. Markdown labels the two source roles as `Primary
Sources` and `Extra Sources`.

### `research`

```console
forager research QUERY [--plan FILE|-] [--budget quick|standard|deep]
                       [--evidence-dir DIR] [--fallback auto|off]
                       [--timeout SECONDS] [--format json|markdown|content]
                       [--output FILE] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Research question passed to the research pipeline. | Required |
| `--plan FILE\|-` | Load a strict Schema v1 plan from `FILE`; use `-` to read it from stdin. Omit it to have the configured classifier generate the plan. | Classifier-generated |
| `--budget BUDGET` | Select `quick`, `standard`, or `deep` execution breadth. | `standard` |
| `--evidence-dir DIR` | Store the plan, fetched evidence, and summary under `DIR`. | A unique temporary directory |
| `--fallback MODE` | Use `auto` or `off` for provider fallback. | `auto` |
| `--timeout SECONDS` | Set the whole research deadline. | `600` |
| `--format FORMAT` | Use `json`, `markdown`, or `content`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--verbose` | Include full provider attempts inline. | Off |

Pipe stdin all the way into the command when using `--plan -`:

```console
printf '%s' "$PLAN_JSON" | forager research "QUERY" --plan - --budget standard --format json
```

Use `research-plan.json` as the Schema v1 shape. A caller-provided plan is authoritative and skips
plan generation. An omitted plan requires a configured classifier. Invalid JSON, unsupported
versions, unknown or missing fields, invalid capabilities, empty decomposition, and empty or
duplicate subquestion IDs exit with code 2.

Default JSON is a Research Evidence Index. Each item in `evidence_items` contains evidence identity
and metadata plus a readable `evidence_items[].path`; fetched body content lives at that path rather
than in stdout. The top level contains `evidence_dir`, `plan_path`, `unconsumed_candidates` as a
count and path, `gap_check`, `capability_gaps`, `synthesis_policy: "fetch_before_claim"`, and the
`journal_ref`/`journal_status` pair. Markdown and content render the same index and unresolved gaps.
On success, the Evidence Index locates these artifacts. On terminal failure, a non-null
`summary_path` points to the readable Research Recovery Manifest containing completed evidence,
gaps, and artifact paths. If it is null, use the other reported paths and gaps before diagnosis.
`--verbose` adds provider attempts inline.

## Direct operations

### `fetch`

```console
forager fetch URL [--timeout SECONDS] [--format json|markdown|content]
                  [--output FILE] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `URL` | Known page or document URL to retrieve through the configured `web_fetch` chain. | Required |
| `--timeout SECONDS` | Set the deadline for the complete fallback chain. | `180` |
| `--format FORMAT` | Use `json`, `markdown`, or `content`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--verbose` | Include full provider attempts inline. | Off |

### `map`

```console
forager map URL [--instructions TEXT] [--max-depth N] [--max-breadth N]
                [--limit N] [--timeout SECONDS] [--format json|markdown]
                [--output FILE] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `URL` | Site root or page from which mapping starts. | Required |
| `--instructions TEXT` | Tell the mapper which pages or structure to prioritize. | Empty |
| `--max-depth N` | Set traversal depth in `1..=5`. | `1` |
| `--max-breadth N` | Set per-level breadth in `1..=500`. | `20` |
| `--limit N` | Set a positive total result limit. | `50` |
| `--timeout SECONDS` | Set the whole-command deadline in `10..=150`. | `150` |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--verbose` | Include full provider attempts inline. | Off |

## Provider-direct commands

Provider-direct commands bypass capability routing and automatic cross-provider fallback. If their
optional `--timeout` is omitted, they use the corresponding `providers.<name>.timeout`
configuration value, which defaults to 30 seconds.

### `exa search`

```console
forager exa search QUERY [--num-results N] [--search-type neural|keyword|auto]
                         [--include-text] [--include-highlights]
                         [--start-published-date DATE]
                         [--include-domains CSV] [--exclude-domains CSV]
                         [--category NAME] [--timeout SECONDS]
                         [--format json|markdown] [--output FILE] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | Exa search query. | Required |
| `--num-results N` | Request between 1 and 100 results. | `5` |
| `--search-type TYPE` | Use `neural`, `keyword`, or `auto` search. | `auto` |
| `--include-text` | Include full result text. | Off |
| `--include-highlights` | Include result highlights. | Off |
| `--start-published-date DATE` | Restrict results to the given lower publication-date bound. | Omitted |
| `--include-domains CSV` | Include only the comma-separated domains. | Empty |
| `--exclude-domains CSV` | Exclude the comma-separated domains. | Empty |
| `--category NAME` | Restrict results to an Exa category. | Omitted |
| `--timeout SECONDS` | Override `providers.exa.timeout`. | Configured value |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--verbose` | Include full provider attempts inline. | Off |

### `exa similar`

```console
forager exa similar URL [--num-results N] [--timeout SECONDS]
                        [--format json|markdown] [--output FILE] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `URL` | URL for which Exa should find similar pages. | Required |
| `--num-results N` | Request between 1 and 100 results. | `5` |
| `--timeout SECONDS` | Override `providers.exa.timeout`. | Configured value |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--verbose` | Include full provider attempts inline. | Off |

### `context7 library`

```console
forager context7 library NAME [QUERY] [--timeout SECONDS]
                                     [--format json|markdown]
                                     [--output FILE] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `NAME` | Library or package name to resolve. | Required |
| `QUERY` | Optional context used to rank matching libraries. | Empty |
| `--timeout SECONDS` | Override `providers.context7.timeout`. | Configured value |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--verbose` | Include full provider attempts inline. | Off |

### `context7 docs`

```console
forager context7 docs LIBRARY_ID QUERY [--timeout SECONDS]
                                       [--format json|markdown|content]
                                       [--output FILE] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `LIBRARY_ID` | Context7-compatible library ID, normally obtained from `context7 library`. | Required |
| `QUERY` | Documentation question or topic. | Required |
| `--timeout SECONDS` | Override `providers.context7.timeout`. | Configured value |
| `--format FORMAT` | Use `json`, `markdown`, or `content`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--verbose` | Include full provider attempts inline. | Off |

### `anysearch search`

```console
forager anysearch search QUERY [--domain DOMAIN --sub-domain SUBDOMAIN]
                               [--sub-domain-params JSON] [--max-results N]
                               [--timeout SECONDS] [--format json|markdown]
                               [--output FILE] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `QUERY` | AnySearch discovery query. | Required |
| `--domain DOMAIN` | Select a parent vertical domain. Must be paired with `--sub-domain`. | Omitted |
| `--sub-domain SUBDOMAIN` | Select a subdomain without dotted shorthand. Must be paired with `--domain`. | Omitted |
| `--sub-domain-params JSON` | Pass a JSON object for the selected subdomain. It requires both domain options and cannot override `query`, `domain`, `sub_domain`, or `max_results`. | Empty object |
| `--max-results N` | Request between 1 and 100 results. | `5` |
| `--timeout SECONDS` | Override `providers.anysearch.timeout`. | Configured value |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--verbose` | Include full provider attempts inline. | Off |

Use no domain options for general vertical discovery. Use separate undotted parent and subdomain
values for scoped discovery. The retired `security.cve` alias is invalid; use
`--domain security --sub-domain vuln`.

### `anysearch domains`

```console
forager anysearch domains DOMAIN [--timeout SECONDS] [--format json|markdown]
                                 [--output FILE] [--verbose]
```

| Argument or option | Meaning | Default |
| --- | --- | --- |
| `DOMAIN` | Undotted parent domain whose supported subdomains should be listed. | Required by the parser |
| `--timeout SECONDS` | Override `providers.anysearch.timeout`. | Configured value |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |
| `--output FILE` | Tee the rendered result to a file. | Omitted |
| `--verbose` | Include full provider attempts inline. | Off |

## Configuration and diagnostics

### `config`

```console
forager config path
forager config list
forager config set KEY VALUE
printf '%s' "$TOML_VALUE" | forager config set KEY -
forager config unset KEY
```

| Command | Behavior |
| --- | --- |
| `config path` | Print the active configuration file path without loading or validating its schema. |
| `config list` | Print the effective JSON view, including each value's source and masked credentials. When parsing fails, report the path, bad key, and location. |
| `config set KEY VALUE` | Set one schema key from a TOML literal. Argument values may remain in shell history. Invalid paths, types, or enum values fail without writing. |
| `config set KEY -` | Read the complete value from stdin; use this form for secrets. |
| `config unset KEY` | Remove only the file-layer value. An environment override can remain effective. |

Use dotted schema paths such as `providers.exa.timeout`. `config set` and `config unset` edit the
document layer and remain available for repairing a schema-invalid but syntactically parseable
file. Edit the path printed by `config path` when the TOML syntax itself is damaged.

### `setup`

```console
forager setup [--non-interactive] [--lang zh|en]
```

| Option | Meaning | Default |
| --- | --- | --- |
| `--non-interactive` | Create a complete commented configuration template without prompting. Refuse to overwrite an existing target. | Off |
| `--lang LANG` | Use `zh` or `en` for the interactive setup prompts. | Prompt/default locale |

After setup, use `forager doctor` to check the resulting configuration.

### `doctor`

```console
forager doctor [--provider PROVIDER] [--timeout SECONDS] [--format json|markdown]
```

| Option | Meaning | Default |
| --- | --- | --- |
| `--provider PROVIDER` | Deep-probe one of `xai`, `openai_compatible`, `tavily`, `firecrawl`, `jina`, `context7`, `exa`, or `anysearch`. Without it, run the shallow all-provider report. | Omitted |
| `--timeout SECONDS` | Set the diagnostic deadline. | `30` |
| `--format FORMAT` | Use `json` or `markdown`. | `json` |

Use `doctor` for credentials, connectivity, provider responses, and effective configuration
inspection. In shallow mode, `ok` covers every configured provider: if any configured provider is
unreachable, the top-level result is false and the command uses exit code 4. Do not use it as the
recovery path for configuration that cannot be loaded.

### `smoke`

```console
forager smoke
forager smoke --live [--timeout SECONDS]
                   [--outage-evidence CASE_ID=EVIDENCE_URL ...]
forager smoke --live --list
```

| Option | Meaning | Default |
| --- | --- | --- |
| `--live` | Run the live acceptance registry instead of offline checks. | Off |
| `--list` | Print the live case registry without running it. Requires `--live`. | Off |
| `--timeout SECONDS` | Set the live run deadline. Requires `--live`. | `180` |
| `--outage-evidence CASE_ID=EVIDENCE_URL` | Attach repeatable provider-outage evidence to a live case. Requires `--live`. | Empty |

The offline form performs local configuration, registry, credential-presence, and journal
writability checks without contacting providers. `smoke` emits JSON. Hidden acceptance-harness
probe flags are internal and are not part of the public CLI.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success, including a valid empty result from a provider-direct command. |
| `2` | Argument or caller-input error, including an invalid plan or invalid `config set` path/value. |
| `3` | Configuration, stdin, or requested output-write error. |
| `4` | Transport/runtime terminal failure, including a hard timeout. |
| `5` | Content-quality or evidence terminal failure. |

Exit code `1` is intentionally unused. Panic status `101` is outside the CLI contract. After a
network command reaches a rendered terminal result, JSON output is emitted as clean JSON on stdout
and diagnostics or logs use stderr. After successful Clap parsing selects JSON, configuration,
stdin, and plan preflight failures use the parseable stdout error object described above. Clap
errors, panics, and non-JSON formats retain their existing channels.
