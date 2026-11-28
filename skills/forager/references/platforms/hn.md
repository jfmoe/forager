# Hacker News

Hacker News is where practitioners discuss software, startups and technical ideas; its comments
are first-hand opinions and experience. It needs no key. Read [`shared.md`](shared.md) for what
every platform shares.

- **What HN says about a topic**: `forager platform hn search 'QUERY'`. `"double quotes"` match a
  phrase and `-word` excludes a word. `--type`, `--author`, `--from`/`--to`, `--min-points` and
  `--min-comments` only when the task implies them; `--type story --min-points 100` keeps the
  discussions many people joined.
- **A discussion**: `forager platform hn fetch 'REF_OR_URL'` writes the item and its whole
  discussion to a file; read it as [`shared.md`](shared.md) says. A discussion can run to a
  million characters: read the start, which holds the most upvoted threads, and only the parts
  you need.
- **Comments inside one big discussion**: search with `--story ID` rather than reading the whole
  file.
- **The HN discussion of a web page**: search its title or URL words with `--type story`, then
  fetch the story.
- **The front page**: `forager platform hn stories`, or `new`, `best`, `ask`, `show`, `job`.
- **The page a story links to**: `fetch --depth full_text` reads it through Web Fetch; a text post
  gives its own body.
- **Evidence**: cite the ref (`hn:8863`) and the `url`. A comment is one user's opinion: attribute
  it to its author and report it as an opinion, never as a fact about the subject.

The step is done when the discussions the task needs are read and each quoted opinion carries its
author and ref.

## Refs

- `hn:<id>`, or a `https://news.ycombinator.com/item?id=<id>` URL; other parameters such as `p=2`
  are ignored. Fetching a comment delivers the discussion below it, with its story.
- User, site and other HN pages are refused (exit 2).

## Result fields

- Items: `ref`, `url`, `depth` (`snippet` for an item with its own text, else `metadata`),
  `title` (`null` for a comment), `authors`, `published`, `item_type`, `link_url`, `points`,
  `comments`, `text` (the item's own body, never its comments), and for a comment `story_ref`,
  `story_title` and `parent_ref`.
- Search and `stories` counts can lag the site by a few minutes; a fetch's counts are live.
- `stories` items add `rank`, the position in the list. Deleted, dead and missing items are
  skipped, so ranks can have gaps; that is expected, not a defect.
- A search reaches its first 1,000 results; past them no cursor is issued.
- A discussion file's `order` is `ranked_top_level` when top-level comments follow HN's rank, or
  `chronological` when the rank was unavailable (stderr says why). Replies are always in time
  order. `comments_delivered` counts the comments in the file.
- A discussion file is not a full mirror of the page: deleted and dead comments are absent, and
  comments from the last few minutes may be missing. When `comments_delivered` is below
  `comments`, that is why.

## Failures

- Exit 4 `parameter` saying the item was not found, or is deleted or dead: report it.
- Exit 4 `parameter` on `--depth full_text` of a comment: fetch it at the default depth.
- Exit 4 `rate_limited`: the search service allows 10,000 requests an hour from one address; wait
  rather than retry.
- Exit 3 at `full_text`: no Web Fetch provider is configured; see
  [`diagnostics.md`](../diagnostics.md).
- Exit 5: the post has neither a title nor text.
