# YouTube

YouTube holds talks, lectures, tutorials and reviews. Read [`shared.md`](shared.md) for what every
platform shares, and Failures below after a non-zero exit. Three evidence layers never stand in for
each other:

- **What the publisher wrote** (title, description, chapters): `forager platform youtube fetch
  'REF_OR_URL' --depth abstract`. Never report it as what the video says or shows.
- **What is said** (one caption track as a transcript): `fetch` at its default `full_text`, through
  the opt-in `youtube_web` route. Not evidence of what the video shows.
- **What a model saw and heard**: `forager platform youtube ask 'REF_OR_URL' 'QUESTION'` for what
  captions cannot say (a slide, code, a chart, who appears), asked once with everything the task
  needs; `forager platform youtube transcribe 'REF_OR_URL'` only when the video has no usable
  caption track. Both are the model's account: attribute them to `model`, check a moment against
  the video before quoting it, and never quote a model transcript as the speaker's exact words.

Costs and routes:

- **Data API key** (`youtube_api`): search, fetch, comments and videos. Search is scarce:
  about 100 a day, failed searches included, so search once with the filters the task implies.
  Fetches, comments and video pages draw on a separate, larger daily budget.
- **Gemini key** (`gemini_api`): `ask` and `transcribe` spend model tokens and take seconds to
  minutes; `usage` reports the spend.
- **Captions are opt-in** (`youtube_web`): off until the user enables it.
  - What it adds: what a video says, as timestamped paragraphs.
  - What it needs: the user runs the `forager config set platforms.youtube.order …` command the
    refusal names, which keeps the routes already there.
  - What it risks: it reads YouTube's undocumented endpoints, against YouTube's terms. YouTube may
    block the user's network address for hours to weeks, and with an official key also enabled,
    Google may suspend the user's Cloud project.
  Tell the user these three things and wait, as the entry's opt-in rule says; meanwhile use
  `--depth abstract`. The route takes about 10 seconds a video; fetch only those needed.

Other operations: `forager platform youtube search 'QUERY'` (`-word` excludes, `a|b` matches
either; `--duration long` keeps talks and lectures); `comments` for what viewers say (opinions,
never facts about the video); `videos` lists a playlist or a channel's uploads.

The step is done when each claim names the evidence layer it rests on.

## Refs

- A video: `youtube:<video_id>`, or a watch, `youtu.be`, `shorts`, `live` or `embed` URL.
  `fetch`, `comments`, `ask` and `transcribe` take one video only.
- A collection for `videos`: `youtube:playlist/<id>`, `youtube:channel/<UC…>`, a playlist URL, a
  watch URL with `list=`, a `/channel/<UC…>` URL or an `@handle` URL.
- `--channel` takes a channel ref (`channel_ref`) or a `/channel/<UC…>` URL, not an
  `@handle`. `/c/` and `/user/` pages are refused.

## Result fields

- Search items carry the channel (`authors`, `channel_ref`), `duration_seconds`, `live_status`,
  `views`, `likes`, `comments` and `has_captions`. All but the channel come from a second request;
  when it fails they are `null` and stderr says so. A count the channel hides is `null`.
- `videos` items add `position`; duration and counts are `null` (fetch a video for them).
  Private and deleted videos are skipped and stderr names their positions.
- Through `youtube_web` alone, `likes` and `comments` are always `null`.
- A transcript's `transcript.source` is `manual` (the publisher's captions), `asr` (YouTube's
  speech recognition, unpunctuated and error-prone) or `model`; say which. The default track is
  the publisher's in the original language, else ASR; `--lang en --lang de` sets the priority;
  machine translations are never used. `caption_tracks` lists the tracks.
- `chapters` stays empty unless the description marks at least three, starting at `0:00`.
- Comments: `has_more` means more top-level comments exist; `replies_complete: false` means a
  comment has replies forager cannot read. `--sort time` can still put a pinned comment first.

## Failures

- Exit 2 on `full_text` naming `youtube_web`: captions need that route; use `--depth abstract`
  and follow the opt-in rule above.
- Exit 5 on `full_text`: no qualifying caption track, and the message lists the languages that
  exist. Retry once with `--lang` for one the user can use; otherwise `transcribe`, saying the
  transcript is a model's. A live stream has no transcript until it ends.
- Exit 5 on `--depth abstract`: the video has no description; fetch `--depth metadata`.
- Exit 3 naming `providers.youtube_api.keys` or `providers.gemini_api.keys`: no key, or (YouTube)
  more than one. Ask the user to run `forager config set <key> -` and type `["THEIR_KEY"]` on
  stdin.
- Exit 4 `auth` from the Data API or Gemini: the key was rejected; ask the user to check it.
- Exit 4 `quota_exhausted` from the Data API: today's quota is spent (it resets at midnight
  Pacific). Tell the user and continue with `forager search`.
- Exit 4 `rate_limited` or `quota_exhausted` from Gemini: tell the user; do not loop.
- Exit 4 `parameter` saying comments are disabled: the video allows none.
- Exit 4 `auth` or `rate_limited` from `youtube_web`, or `runtime` saying the caption track was
  empty: YouTube is restricting the network address. Stop fetching captions, tell the user to
  wait or switch networks, and do not retry.
- Exit 5 on `ask` or `transcribe`: a live stream, an answer cut off (the video is too long), no
  valid transcript, or a safety filter. Report it; do not repeat it.

## Video files

forager downloads no media. Only when the user explicitly wants the file, run `yt-dlp
--ignore-config --no-playlist -o 'DIR/%(id)s.%(ext)s' -- 'VIDEO_URL'` (add `-x` for audio only)
for the videos they named, without cookies. Downloading is against YouTube's terms and risks the
network address.
