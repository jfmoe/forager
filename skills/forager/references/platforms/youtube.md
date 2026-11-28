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
  about 100 a day, failed searches included, so search once with the filters the task implies;
  the other operations cost far less.
- **Gemini key** (`gemini_api`): `ask` and `transcribe` spend model tokens and take seconds to
  minutes; `usage` reports the spend.
- **Captions are opt-in** (`youtube_web`), as the entry's opt-in rule says; meanwhile use `--depth
  abstract`.
  - What it adds: what a video says, as timestamped paragraphs.
  - What it needs: the user runs the `forager config set` command the refusal names.
  - What it risks: it reads YouTube's undocumented endpoints, against YouTube's terms. YouTube may
    block the user's network address for hours to weeks, and with an official key also enabled,
    Google may suspend the user's Cloud project.
  It takes about 10 seconds a video; fetch only those needed.

Other operations: `search` (`--duration long` keeps talks and lectures); `comments` for what
viewers say (opinions, never facts about the video); `videos` lists a playlist or a channel's
uploads. Refs: `youtube:<video_id>` or a video URL; `youtube:playlist/<id>`,
`youtube:channel/<UC…>` or their URLs for `videos`.

The step is done when each claim names the evidence layer it rests on.

## Result fields

- A search item's duration, counts and `has_captions` are `null` when the channel hides them or
  their request failed (stderr says which). `videos` items add `position` but no duration or
  counts; fetch a video for them. Through `youtube_web` alone, `likes` and `comments` are `null`.
- A transcript's `transcript.source` is `manual` (the publisher's captions), `asr` (YouTube's
  speech recognition, unpunctuated and error-prone) or `model`; say which. `caption_tracks` lists
  the tracks.
- `chapters` stays empty unless the description marks at least three, starting at `0:00`.
- Comments: `has_more` means more top-level comments exist; `replies_complete: false` means a
  comment has replies forager cannot read.

## Failures

- Exit 2 on `full_text` naming `youtube_web`: captions need that route; use `--depth abstract`
  and follow the opt-in rule.
- Exit 5 on `full_text`: no qualifying caption track; the message lists the languages that exist.
  Retry once with `--lang` for one the user can use; otherwise `transcribe`, saying the transcript
  is a model's. A live stream has no transcript until it ends.
- Exit 5 on `--depth abstract`: the video has no description; fetch `--depth metadata`.
- Exit 4 `quota_exhausted` from the Data API: today's quota is spent (it resets at midnight
  Pacific). Tell the user and continue with `forager search`.
- Exit 4 `rate_limited` or `quota_exhausted` from Gemini: tell the user; do not loop.
- Exit 4 `auth` or `rate_limited` from `youtube_web`, or `runtime` saying the caption track was
  empty: YouTube is restricting the network address. Stop fetching captions, tell the user to
  wait or switch networks, and do not retry.
- Exit 5 on `ask` or `transcribe`: a live stream, an answer cut off (the video is too long), no
  valid transcript, or a safety filter. Report it; do not repeat it.

## Video files

forager downloads no media. Only when the user explicitly wants the file, run `yt-dlp
--ignore-config --no-playlist -o 'DIR/%(id)s.%(ext)s' -- 'VIDEO_URL'` (`-x` for audio only) for
the videos they named, without cookies. Downloading is against YouTube's terms and risks the
network address.
