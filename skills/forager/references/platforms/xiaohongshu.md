# Xiaohongshu 小红书

Xiaohongshu (RED) holds Chinese consumer reviews and lifestyle, travel and shopping experience,
written as personal notes. Read [`shared.md`](shared.md) for what every platform shares, and
Failures below after a non-zero exit.

- **Opt-in**: its only route, `xiaohongshu_browser`, is off until the user enables it. It runs in
  a headless browser that forager drives, logged into the user's Xiaohongshu account.
  - What it adds: notes, their comments and counts, which general search does not reach.
  - What it needs: the user runs `forager browser install` (a package exists only for macOS on
    Apple silicon; elsewhere `browser.path` names a local Chromium-based browser), then `forager
    browser login xiaohongshu` and logs in in the window, then `forager config set
    platforms.xiaohongshu.order '["xiaohongshu_browser"]'`, and `forager doctor` to confirm.
  - What it risks: Xiaohongshu forbids scraping and limits accounts that request too much; every
    search, fetch and comment read runs under the user's account. Suggest a secondary account.
  Tell the user these three things and wait for their answer, as the entry's opt-in rule says.
- **Notes on a topic**: `forager platform xiaohongshu search 'QUERY'`. `--sort`, `--note-type`
  and `--publish-time` only when the task implies them. Items are cards, never the note body.
- **One note**: `forager platform xiaohongshu fetch 'ACCESS_URL'` with a search item's
  `access_url`, unchanged. It writes the note's text, tags and image links to a file. A link
  without its `xsec_token`, or a bare ref, cannot open: search for the note and fetch its
  `access_url`. `forager fetch` cannot read a note.
- **What readers say**: `forager platform xiaohongshu comments 'ACCESS_URL'`. Leave `--replies`
  at its default unless the task is about the replies under particular comments; each expanded
  comment is one more click in the user's account.
- **Pace**: one browser command every 10 seconds, never retried. Keep the number of commands small,
  and fetch soon after searching: a token works for that note in that session for a while, not
  indefinitely.
- **Evidence**: cite the ref (`xiaohongshu:64a1b2c3d4e5f60718293a4b`) with the title, and give the
  user the `access_url`; the canonical `url` does not open without the token. A card supports
  claims about title, author, date and counts only. A note is one person's post: attribute it,
  and say that its images and videos were not read. Comments are readers' opinions; say how many
  you read.

The step is done when the notes the task needs are fetched and each claim is attributed to its
author.

## Refs

- A search item's `access_url`, or a full xiaohongshu.com note URL with its `xsec_token`.
- `xiaohongshu:<note_id>` names a note but cannot open it.
- Short links and `rednote.com` links are refused (exit 2).

## Result fields

- Search items: `ref`, `url`, `depth: metadata`, `title` (may be `""`), `authors`, `published`,
  `published_text`, `note_type`, `author_id`, `likes`, `collects`, `comments`, `shares` and
  `access_url`.
- A card's relative time (`3天前`, `昨天 21:38`) is turned into `published` from this machine's
  clock, so it is right to the day at best; quote `published_text` when the wording matters or
  `published` is `null`.
- Counts are Xiaohongshu's display text, such as `1.2万`, not exact numbers.
- Results are personalized and change between runs; never present them as a stable ranking.
- `next_cursor` is always `null`: a search cannot continue. When stderr says more results remain,
  rerun once with a larger `--limit`.
- A fetch has exact Beijing times in `published` and `updated`, plus `tags`, `images`, `video`
  (length and size; the video is never downloaded) and `ip_location`. Its `access_url` holds the
  token; `ref` and `url` never do.
- Comment text keeps emoji codes such as `[doge]` as written.
- Comments carry `has_more` for the note and `replies_has_more` per comment; when `has_more` is
  true and more comments matter, rerun once with a larger `--limit`.

## Failures

- Exit 3 naming `platforms.xiaohongshu.order`: the route is off; follow the opt-in rule above.
- Exit 3 saying `xiaohongshu_browser cannot run`: the browser is missing or outdated, the host
  has no package, or the user never logged in. The message names the fix; tell the user.
- Exit 4 `auth`: the session is logged out, Xiaohongshu wants a verification, or it says the
  account requests too often. Ask the user to run `forager browser login xiaohongshu` and finish
  the check by hand, then retry once; after "requests too often", wait first.
- Exit 4 `parameter` naming `300031`, `300017` or a security restriction on a search: Xiaohongshu
  is limiting the account. Stop using Xiaohongshu for this task, tell the user, do not retry.
- The same codes on a fetch or comments read: the token may be stale, the note restricted or
  removed, or the account limited. Search once more and retry with the new `access_url`; when that
  also fails, stop and tell the user.
- Exit 4 `runtime` saying the replies of a comment could not be expanded: retry once without
  `--replies` and say the replies were not read.
- Exit 4 `runtime` saying the Browser Profile is in use: a login window or another forager
  command holds it; wait for it to end, then retry once.
- Exit 4 `timeout`: retry once with `--timeout 180`. `network`: retry once after a pause, then ask
  the user to check this machine's network.
- Exit 5: the note has no title, text or images.
