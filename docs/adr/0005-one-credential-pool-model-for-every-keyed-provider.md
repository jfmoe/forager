# Provider Credential Pool for every provider that needs credentials

forager uses a **Provider Credential Pool** as the only credential model for every provider whose registration requires credentials: xai, openai_compatible, exa, tavily, jina, firecrawl, context7, and anysearch. A single credential is the one-element case of the same pool. A provider that requires no credentials, such as arxiv_api, has no pool: it never claims or rotates a credential and is always configured.

**Config:** every provider that needs credentials has one TOML `keys` array containing real string values. Empty values are removed and duplicates are discarded while preserving order. There is no `KEY`/`KEYS` dual shape, JSON-encoded array, replacement priority, or provider allowlist. The section of a provider that needs no credentials has no `keys` leaf; it has only `url` and `timeout`. The classifier uses the same `keys` shape and pool behavior.

**Selection and failure:** claim `next_index` under the XDG state cursor-file lock at request start and advance immediately without rollback. On RateLimited or QuotaExhausted, rotate before retrying and try each credential at most once in the request; do not back off and retry the same credential for HTTP 429. Other errors follow the provider's retry and capability fallback rules.

**Observability:** configuration and doctor views may represent each configured credential with the same fixed, irreversible mask and report `configured`, `key_count`, and `source`. No output or error may contain raw credential characters. Journals and cursor state store neither raw credentials nor masked placeholders; cursor state contains only schema-versioned, non-sensitive indices. Runtime diagnostics may report rotation and credential-index facts.
