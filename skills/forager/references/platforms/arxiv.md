# arXiv

arXiv holds preprints in physics, mathematics, computer science, quantitative finance and nearby
fields, with full text. It needs no key. Read [`shared.md`](shared.md) for what every platform
shares.

- **Papers on a topic**: `forager platform arxiv search 'QUERY'`. arXiv matches literal words,
  not meaning, and every term must match: use a few distinctive keywords and quote multi-word
  concepts, such as `'"world model" robot manipulation'`. Scattered words match unrelated papers.
  For an open-ended question, find papers with `forager search` first, then read them here.
- **Filters**: `--category`, `--author`, `--title`, the submission dates and `--sort`, only when
  the task states or implies them. A search needs a query or one of `--category`, `--author`,
  `--title`.
- **One paper**: `forager platform arxiv fetch 'REF_OR_URL'`. It writes the full text to a file
  and prints metadata with `content_path`; read it as [`shared.md`](shared.md) says. Add
  `--depth abstract` when the abstract answers the task: it reads no body and is faster.
- **Evidence**: cite the versioned ref, such as `arxiv:2401.04088v1`. An abstract is not the full
  text; say which one a claim rests on. Make no claim about authors, affiliations or categories
  beyond the returned metadata.
- **Pace**: arXiv takes one request every 3 seconds, shared by every forager process on the
  machine, so a batch of fetches queues. Fetch only the papers the task needs.

The step is done when each paper the task needs is read at the depth the answer rests on.

## Refs

- `arxiv:<id>` or `arxiv:<id>v<n>`, with a new-style ID (`2401.01234`) or an old-style one
  (`hep-th/9901001`).
- arxiv.org `abs`, `pdf` and `html` URLs, and `export.arxiv.org` URLs.
- Without a version, forager reads arXiv's current version and returns its versioned ref.

## Result fields

- Search items: `ref`, `url`, `title`, `authors`, `published`, the full `abstract`, `updated`,
  `primary_category`, `categories`, `doi`, `journal_ref`, `comment` and `pdf_url`. A field arXiv
  lacks is `null`.
- `next_cursor` continues the search: pass it alone with `--cursor`, without the query or options.
  It is `null` on the last page; an empty `items` with exit 0 means no paper matched.
- A full-text fetch adds `content_path`, `content_len` and `content_url`, which says whether the
  body came from the paper's HTML or its PDF.

## Failures

- Exit 2: forager refused the input before sending it; the message names the problem, such as a
  short link, a reversed date range, or a search without a query or filter. Fix it and rerun.
- Exit 3 at `full_text`: no Web Fetch provider is configured. Fetch with `--depth abstract`, or go
  to [`diagnostics.md`](../diagnostics.md).
- Exit 4 `parameter`: arXiv has no such paper, or rejected the query; its message is passed on.
  Check the ref rather than retrying.
- Exit 4 `rate_limited`: arXiv refused the pace. Wait before the next request; do not retry in a
  loop.
- Exit 5: both the HTML and the PDF were too thin to read. Report it; answer from
  `--depth abstract` only when the abstract still answers the task, and say so.
