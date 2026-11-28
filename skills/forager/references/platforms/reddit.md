# Reddit

Reddit holds community discussion: user experience, recommendations and troubleshooting, ranked by
votes. Read [`shared.md`](shared.md) for what every platform shares, and Failures below after a
non-zero exit.

- **Opt-in**: Reddit's only route, `reddit_browser`, is off until the user enables it, as the
  entry's opt-in rule says. It reads Reddit in forager's headless browser, logged into the user's
  own Reddit account.
  - What it adds: Reddit posts and their discussions with scores, which general search reaches
    only in part.
  - What it needs: the user runs `forager browser install`, then `forager browser login reddit`
    and logs in in the window it opens, then adds `reddit_browser` to `platforms.reddit.order`.
  - What it risks: Reddit's User Agreement does not allow automated access, and the risk falls on
    the user's Reddit account.
  Until the user enables it, use `forager search` for Reddit content.
- **What Reddit says about a topic**: `forager platform reddit search 'QUERY'`, in Reddit's own
  search syntax. `--subreddit`, `--sort` and `--time` only when the task implies them; `--time`
  applies to `relevance`, `top` and `comments`, not to `hot` or `new`.
- **A post and its discussion**: `forager platform reddit fetch 'REF_OR_URL'` writes the post and
  the comments its page loaded (about the first 100, in top order) to a file; read it as
  [`shared.md`](shared.md) says.
- **Pace**: one browser command every 10 seconds. A search reads one screenful of posts,
  scrolling for more, and cannot continue later: when stderr says Reddit has more, narrow the
  query, subreddit or time window rather than paging. Keep the number of commands small.
- **Evidence**: cite the ref (`reddit:17snc7d`) with the subreddit, and give the user the
  `permalink`. A post or comment is one user's statement: attribute it and report it as an
  opinion, never as a fact about the subject. Say how many comments you read.

The step is done when the discussions the task needs are read and each quoted opinion carries its
author and ref.

## Refs

- `reddit:<id>` or `reddit:t3_<id>`; a reddit.com post URL (`www`, `old`, `new`, `np` or `m`);
  a `redd.it/<id>` short link.
- A comment link is refused (exit 2) and the message names its post's ref; fetch that.
- A share link (`/s/…`) is refused (exit 2): ask the user for the post URL it opens.

## Result fields

- Search items: `ref`, `url`, `depth` (`snippet` when Reddit showed an excerpt, else `metadata`),
  `title`, `authors`, `published`, `subreddit`, `permalink`, `link_url`, `score`, `comments`,
  `nsfw`, `spoiler`, `snippet` and `snippet_source`. With `snippet_source: comment` the excerpt
  is from a comment, not the post's text.
- `next_cursor` is always `null`: a search cannot continue.
- A fetch adds `post_type` and `text`; at the default depth also `content_path`,
  `comments_delivered`, `has_more_comments` and `order`, the comment order Reddit applied (`top`
  unless Reddit chose another).
- `comments` is the post's whole count. It can exceed `comments_delivered` even when
  `has_more_comments` is false, because Reddit does not render every removed comment. A deleted
  comment keeps its place as `*[deleted or removed]*`.
- Scores and counts are Reddit's at the time of the request; results are Reddit's ranking, not a
  complete or stable view.

## Failures

- Exit 3 naming `platforms.reddit.order`: the route is off; follow the opt-in rule above.
- Exit 3 saying `reddit_browser cannot run`: the browser is missing or the user never logged in.
  Tell the user the command the message names; only they can complete the login.
- Exit 4 `auth` (a verification page, HTTP 403 or 429, or a logged-out session): stop using Reddit
  for this task and do not retry. Tell the user to run `forager browser login reddit` and deal
  with what the site shows in its window, or to wait. Never try to pass a verification page.
- Exit 4 `runtime` saying the page format may have changed: report it and continue with `forager
  search`. `runtime` for an HTTP 5xx from Reddit: retry later.
- Exit 4 `parameter` saying the post was not found or is not available: it may be removed,
  private, restricted, or hidden by the account's adult-content setting. Report it.
- Exit 5 on `--depth full_text`: the post has no text or link, such as an image, gallery or
  crosspost. Fetch at the default depth to read its discussion.
