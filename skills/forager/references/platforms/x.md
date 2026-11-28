# X

X (Twitter) is where accounts post news, announcements and reactions as they happen. forager reads
it through a model that searches X with the xAI key of main search, so no other setup is needed.
Read [`shared.md`](shared.md) for what every platform shares, and Failures below after a non-zero
exit.

- **Every X text is relayed**: the model reports what it read, and xAI never returns the posts
  themselves, so `text`, `author_handle` and an `ask` answer may be reworded, translated or cut.
  Never present them as a verbatim quote. When the exact wording matters (a quote, a number, a
  claim attributed to a person), run `forager fetch` on the post's status URL, which returns the
  text verbatim, or tell the user to open the post.
- **What X says, or what given accounts or days say**: `forager platform x ask 'QUESTION'`. Prefer
  one `ask` over many searches when the user wants an answer.
- **Posts to list or cite**: `forager platform x search 'QUERY'`, one page of posts and no
  further page; rewrite the query or narrow the filters for others.
- **One post with its replies**: `forager platform x fetch 'REF_OR_URL'`.
- **An X article** (`/article/` in the URL): `forager fetch 'URL'`, which returns its text.
- **A general question X may inform**: keep `forager search`; main search already reads X.
- **Filters**: `--handle` (repeatable) for given accounts, `--exclude-handle` to drop accounts,
  `--from`/`--to` for UTC days, only when the task implies them. There is no sort: the model picks
  top or latest itself.
- **Cost**: the model reads as many posts as it decides, and the official xAI API bills each post
  read.
- **Evidence**: cite the ref (`x:2110126570233094527`) with the author's handle, and give the user
  the `url`. A post is one account's statement: attribute it. Results show what the model chose to
  read, not a complete or ranked view of X.

The step is done when the posts or answer the task needs are read and every quoted wording is
checked or marked as relayed.

## Refs

- `x:<post_id>`, or an x.com or twitter.com post URL: `/<handle>/status/<id>`, `/i/status/<id>`
  or `/i/web/status/<id>`, with or without `/photo/N`, a query or a fragment.
- A `t.co` short link is refused (exit 2): ask the user for the x.com URL it opens.
- Article URLs (`/<handle>/article/<id>`, `/i/article/<id>`, `/status/<id>/article`) are refused
  (exit 2); fetch them with `forager fetch`.

## Result fields

- Search items: `ref`, `url`, `depth: snippet`, `title: null`, `authors`, `published`,
  `author_handle` and `text`. `published` comes from the post ID, not the model; it is `null` for
  posts from before November 2010.
- Posts the model did not cite as read, and posts outside `--from`/`--to`, are dropped with one
  stderr note; fewer posts than `--limit` can mean that. An empty list means the model found none.
- A fetch's `thread` lists every post with its `role`: `parent`, `root`, `reply` or `quoted`. The
  root is checked to be the requested post; replies are not, so treat them with more caution.
- An `ask` returns `answer` and `sources`; a cited post has a `ref`, a profile or other page has
  `ref: null`. An answer without sources is unverified.
- A verbatim `forager fetch` of x.com may fail while Jina refuses anonymous access to x.com for a
  while; its attempt names the unblock time. A configured Jina key is more reliable.

## Failures

- Exit 3 naming `providers.xai.keys`: no xAI key; see [`diagnostics.md`](../diagnostics.md).
- Exit 3 naming `platforms.x.order`: the user turned X off. Leave it off; use `forager search`.
- Exit 4 `runtime` saying no post could be verified, or the posts are not the requested JSON:
  retry once; when it fails again, use `ask` or `forager search` and say so.
- Exit 4 `parameter` saying the post was not found or is not visible: it may be deleted, private,
  or out of the model's reach. Report it rather than retrying.
- Exit 4 `rate_limited` or `quota_exhausted`: every xAI key is out; tell the user.
- Exit 5: xAI stopped the answer at a content filter or the output limit. Report it.
