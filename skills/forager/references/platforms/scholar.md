# Google Scholar

Google Scholar finds papers across publishers, counts citations, and lists the papers that cite a
given paper, which arXiv and SSRN cannot. It holds no paper body. It runs on the user's own SerpApi
key. Read [`shared.md`](shared.md) for what every platform shares, and Failures below after a
non-zero exit.

- **Cost**: every search page, fetch and cited-by page spends one search of the key's monthly
  quota (250 on SerpApi's free plan), including an empty page and a ref that does not exist. The
  same request repeated within an hour costs nothing. `forager doctor --provider serpapi` costs
  nothing and reports each key's searches left; run it when the user asks about the quota.
- **Search once**: `forager platform scholar search 'QUERY'` at the default `--limit`, and pick
  candidates from that page. Narrow the query or the years rather than paging with
  `--cursor`.
- **Query syntax**: the query goes to Google Scholar unchanged. Quote phrases; `author:"Surname"`
  restricts the whole query, both sides of `OR` included:
  `'"time series momentum" author:"Pedersen"'`, `'trend OR reversal author:"Moskowitz"'`.
  `--year-from`, `--year-to` and `--review-only` only when the task implies them.
- **Who cites a paper**: `forager platform scholar cited-by 'REF_OR_URL'` traces work forward:
  follow-ups, replications, rebuttals. A well-cited paper has thousands of citing works, so narrow
  first with `--query`, the years, or `--sort date` for the most recently indexed.
- **Versions**: `forager platform scholar fetch 'REF'` lists the versions Google Scholar groups;
  fetch only when you need a version's link, such as a readable copy.
- **Read a paper**: take a `link`, `versions[].link` or `resources[].url` exactly as returned. An
  arxiv.org URL goes to `forager platform arxiv fetch`, an SSRN URL to `forager platform ssrn
  fetch`, any other URL, or one those reject with exit 2, to `forager fetch`. Build no arXiv or
  SSRN ref from a title, byline or snippet.
- **Evidence**: cite the ref (`scholar:18208131694456651388`) and the title. A `snippet` supports
  no claim about findings. `cited_by` measures attention, not quality.

The step is done when the candidates or citing papers the task needs are chosen and each paper
the answer rests on is read from its source.

## Refs

- `scholar:<cluster_id>`, or `https://scholar.google.com/scholar?cluster=<cluster_id>`.
- Citation lists, author profiles and other Scholar pages are refused (exit 2).

## Result fields

- Items: `ref`, `url`, `depth` (`snippet`, or `metadata` without an excerpt), `title`,
  `authors`, `published` (the year), `snippet`, `link`, `source`, `cited_by`, `version_count`,
  `resources` (`{title, file_format, url}` copies) and `result_type`.
- `authors` and `source` copy Google Scholar's byline: names are often initials and the list may
  end in `…`. Never present it as the complete author list.
- Results without a verifiable cluster ID are skipped and named on stderr.
- `next_cursor` is `null` before result 1,000, the most Google Scholar serves.
- A fetch lists at most 20 `versions` in Google Scholar's order; the first is often not the
  published version, so choose by `link` and `source`. That count is not `version_count`, which
  is the search's figure; report neither as the other. stderr says when more versions exist.
- With `--sort date`, each `snippet` starts with an indexing age such as `6 days ago - `: when
  Google Scholar indexed the citation, not when the paper was published.
- An empty cited-by list means nobody cites the paper or Google Scholar does not know the ref.
  Check the ref with `fetch` before reporting no citations.

## Failures

- Exit 4 `quota_exhausted`: every key has used its month's searches. Tell the user and continue
  with `forager search`, arXiv or SSRN; do not retry Scholar.
- Exit 4 `rate_limited`: SerpApi's hourly limit. Tell the user and retry later, not in a loop.
- Exit 4 `parameter` saying Google Scholar has no cluster: the ref is wrong or gone.
- Exit 2 on `--sort date` with years: Google Scholar ignores years when it sorts by date; drop one.
- A cited-by cursor works only with `cited-by`, a search cursor only with `search`.
