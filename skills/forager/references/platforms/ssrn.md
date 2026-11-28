# SSRN

SSRN holds working papers in finance, economics, law and other social sciences. forager reads
SSRN's records in Crossref, so it returns metadata and the abstract when Crossref has one, never
the full text. It needs no key. Read [`shared.md`](shared.md) for what every platform shares.

- **Papers on a topic**: `forager platform ssrn search 'QUERY'`. Matching follows Crossref; a
  title query (`--scope title`) does not need an exact phrase.
- **Typed criteria**: title, author, affiliation, dates, identifiers and ranking each have a flag
  (`forager platform ssrn search --help`); use them for what the task states, such as
  `forager platform ssrn search 'dual momentum' --scope title --author Antonacci --has-abstract`.
- **Criteria that can hide papers**: affiliation, ORCID and funder match only what authors
  deposited, so a paper without that metadata drops out. Say so when such a filter returns little.
- **Three kinds of date**: `--published-*` is the publication date, `--created-*` when Crossref
  registered the DOI, `--updated-*` when the record changed. None of them is SSRN's own posting
  or revision date.
- **One paper**: `forager platform ssrn fetch 'REF_OR_URL'` returns its metadata with the abstract
  when Crossref has one. `--depth abstract` insists on the abstract. For the full text, give the
  user the abstract page `url`.
- **Evidence**: cite the ref (`ssrn:2042750`) and the abstract page `url`. SSRN refs carry no
  version. Use only URLs forager returns; a download link is never derived from the ID.
- **Pace**: one request a second, shared by every forager process on the machine.

The step is done when each paper the task needs is read with its abstract, or reported as having
none.

## Refs

- `ssrn:<id>`, such as `ssrn:2042750`.
- `https://papers.ssrn.com/sol3/papers.cfm?abstract_id=<id>`, `https://ssrn.com/abstract=<id>`
  and `https://www.ssrn.com/abstract=<id>`.
- The DOI `10.2139/ssrn.<id>` or its `https://doi.org/` URL.
- SSRN download links and short links are refused (exit 2); pass the abstract page.

## Result fields

- Items: `ref`, `url` (the abstract page), `depth` (`abstract` with an abstract, else
  `metadata`), `title`, `authors`, `published`, `abstract`, `doi`, `crossref_type` and
  `crossref_created`. A field Crossref lacks is `null`.
- `published` keeps Crossref's precision: `2012`, `2012-04` or `2012-04-19`. Quote it at that
  precision.
- `crossref_created` is when Crossref registered the DOI, never when SSRN posted the paper.
- Records whose DOI is not an SSRN DOI are dropped and named on stderr; fewer items than
  `--limit` can mean that, not the end.
- Paging stops at result 10,000, where `next_cursor` becomes `null` even if Crossref holds more;
  narrow the criteria instead.

## Failures

- Exit 2: a blank query, an invalid date or a range that ends before it starts, a cursor passed
  with any criterion or `--limit` (even its default value), a download link, or `--depth
  full_text`. Fix the input.
- Exit 4 `parameter` saying the paper was not found in Crossref: the paper may still exist on
  SSRN. Say that Crossref has no record of it, not that the paper does not exist.
- Exit 4 `rate_limited`: Crossref refused the pace; wait before the next request.
- Exit 5 on `--depth abstract`: Crossref has no abstract for the paper. Fetch at the default depth
  and say the abstract is unavailable.
