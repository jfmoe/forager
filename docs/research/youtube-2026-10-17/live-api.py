"""Read-only YouTube Data API v3 checks with a user-owned key; run with python3 -I.

The key is read from the macOS keychain and sent only in the X-Goog-Api-Key
header, so it never appears in argv, URLs, or the evidence file.
"""
import datetime
import json
from pathlib import Path
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parent
OUT = ROOT / "official-api-live.json"
BASE = "https://www.googleapis.com/youtube/v3/"
KEYCHAIN_SERVICE = "forager-youtube-data-api"
SAMPLES = ["jNQXAC9IVRw", "Ks-_Mh1QhMc", "eMlx5fFNoYc"]
MISSING_ID = "zzQq9ZxZ9Zq"
HANDLE = "@3blue1brown"
PUBLIC_PLAYLIST = "PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab"
COMMENTS_OFF_CANDIDATES = ["XqZsoesa55w"]
SEARCH_BUDGET = 10
OTHER_BUDGET = 50
COST = {"search": 1, "videos": 1, "commentThreads": 1, "comments": 1,
        "channels": 1, "playlistItems": 1}

KEY = ""
records = []
spent = {"search_queries": 0, "other_units": 0}


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def scrub(text):
    if KEY:
        text = text.replace(KEY, "<redacted-key>")
    text = re.sub(r"projects/\d+", "projects/<redacted>", text)
    text = re.sub(r"(?i)project(\s+(?:number\s+)?)\d+", r"project\1<redacted>", text)
    return text


def call(label, endpoint, params, key=None, summarize=None):
    bucket = "search_queries" if endpoint == "search" else "other_units"
    limit = SEARCH_BUDGET if endpoint == "search" else OTHER_BUDGET
    if key is None and spent[bucket] + COST[endpoint] > limit:
        raise SystemExit(f"budget exceeded before {label}")
    url = BASE + endpoint + "?" + urllib.parse.urlencode(params)
    request = urllib.request.Request(url, headers={
        "X-Goog-Api-Key": KEY if key is None else key,
        "Accept-Encoding": "identity", "User-Agent": "forager-research (gzip)"})
    started = now()
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            status, body = response.status, response.read().decode()
    except urllib.error.HTTPError as error:
        status, body = error.code, error.read().decode()
    if key is None:
        spent[bucket] += COST[endpoint]
    payload = json.loads(body) if body else None
    record = dict(label=label, endpoint=endpoint, params=params, checked_at=started,
                  http_status=status, quota_bucket=bucket if key is None else "not-own-project",
                  documented_cost=COST[endpoint])
    if status >= 400 or payload is None:
        record["error_body"] = json.loads(scrub(body)) if body else None
    else:
        record["page_info"] = payload.get("pageInfo")
        record["next_page_token_present"] = "nextPageToken" in payload
        record["prev_page_token_present"] = "prevPageToken" in payload
        record["top_level_keys"] = sorted(payload)
        if summarize:
            record["observed"] = summarize(payload)
    records.append(record)
    print(label, status, flush=True)
    return status, payload


def search_items(payload):
    return [dict(videoId=i["id"].get("videoId"), kind=i["id"].get("kind"),
                 channelId=i["snippet"]["channelId"],
                 publishedAt=i["snippet"]["publishedAt"],
                 title=i["snippet"]["title"],
                 liveBroadcastContent=i["snippet"].get("liveBroadcastContent"))
            for i in payload.get("items", [])]


def video_items(payload):
    out = []
    for i in payload.get("items", []):
        s, c, st = i.get("snippet", {}), i.get("contentDetails", {}), i.get("status", {})
        out.append(dict(
            id=i["id"], parts_present=sorted(k for k in i if k not in ("kind", "etag", "id")),
            publishedAt=s.get("publishedAt"), channelId=s.get("channelId"),
            title=s.get("title"), description_length=len(s.get("description", "")),
            defaultLanguage=s.get("defaultLanguage"),
            defaultAudioLanguage=s.get("defaultAudioLanguage"),
            liveBroadcastContent=s.get("liveBroadcastContent"),
            duration=c.get("duration"), caption=c.get("caption"),
            definition=c.get("definition"), licensedContent=c.get("licensedContent"),
            statistics=i.get("statistics"), status=st,
            liveStreamingDetails=i.get("liveStreamingDetails")))
    return out


def thread_items(payload):
    out = []
    for t in payload.get("items", []):
        top = t["snippet"]["topLevelComment"]["snippet"]
        replies = t.get("replies", {}).get("comments", [])
        out.append(dict(id=t["id"], totalReplyCount=t["snippet"]["totalReplyCount"],
                        canReply=t["snippet"].get("canReply"),
                        replies_returned=len(replies),
                        top_publishedAt=top.get("publishedAt"),
                        top_likeCount=top.get("likeCount"),
                        top_text_length=len(top.get("textOriginal", ""))))
    return out


def playlist_items(payload):
    return [dict(position=i["snippet"]["position"],
                 videoId=i["contentDetails"]["videoId"],
                 videoPublishedAt=i["contentDetails"].get("videoPublishedAt"),
                 snippet_publishedAt=i["snippet"]["publishedAt"],
                 title=i["snippet"]["title"])
            for i in payload.get("items", [])]


def shift(stamp, **delta):
    moment = datetime.datetime.fromisoformat(stamp.replace("Z", "+00:00"))
    return (moment + datetime.timedelta(**delta)).strftime("%Y-%m-%dT%H:%M:%SZ")


def main():
    # Channel and uploads first: the date-boundary searches need a known publishedAt.
    _, ch = call("channels.forHandle", "channels",
                 dict(part="id,snippet,contentDetails,statistics", forHandle=HANDLE),
                 summarize=lambda p: [dict(id=i["id"], title=i["snippet"]["title"],
                                           customUrl=i["snippet"].get("customUrl"),
                                           uploads=i["contentDetails"]["relatedPlaylists"].get("uploads"),
                                           statistics=i.get("statistics"))
                                      for i in p.get("items", [])])
    channel_id = ch["items"][0]["id"]
    uploads = ch["items"][0]["contentDetails"]["relatedPlaylists"]["uploads"]
    _, up = call("playlistItems.uploads.page1", "playlistItems",
                 dict(part="snippet,contentDetails", playlistId=uploads, maxResults=5),
                 summarize=playlist_items)
    call("playlistItems.public_playlist.page1", "playlistItems",
         dict(part="snippet,contentDetails", playlistId=PUBLIC_PLAYLIST, maxResults=5),
         summarize=playlist_items)

    call("videos.batch_samples", "videos",
         dict(part="snippet,contentDetails,statistics,status,liveStreamingDetails",
              id=",".join(SAMPLES)), summarize=video_items)
    call("videos.missing_id", "videos", dict(part="snippet", id=MISSING_ID),
         summarize=video_items)
    call("videos.mixed_existing_and_missing", "videos",
         dict(part="id", id=f"{SAMPLES[0]},{MISSING_ID}"),
         summarize=lambda p: [i["id"] for i in p.get("items", [])])

    for order in ("relevance", "time"):
        call(f"commentThreads.{order}", "commentThreads",
             dict(part="snippet,replies", videoId=SAMPLES[0], maxResults=20,
                  order=order, textFormat="plainText"), summarize=thread_items)
    # Find a thread whose embedded replies are fewer than totalReplyCount, then page them.
    relevance = next(r for r in records if r["label"] == "commentThreads.relevance")
    partial = [t for t in relevance.get("observed", [])
               if t["totalReplyCount"] > t["replies_returned"]]
    if partial:
        target = partial[0]
        call("comments.replies_of_partial_thread", "comments",
             dict(part="snippet", parentId=target["id"], maxResults=100, textFormat="plainText"),
             summarize=lambda p: dict(parent_id=target["id"],
                                      totalReplyCount=target["totalReplyCount"],
                                      embedded_replies=target["replies_returned"],
                                      returned=len(p.get("items", []))))
    for vid in COMMENTS_OFF_CANDIDATES:
        call(f"videos.comments_off_candidate.{vid}", "videos",
             dict(part="snippet,status,statistics", id=vid), summarize=video_items)
        call(f"commentThreads.comments_off_candidate.{vid}", "commentThreads",
             dict(part="snippet", videoId=vid, maxResults=1), summarize=thread_items)

    query = "python tutorial"
    _, p1 = call("search.page1", "search",
                 dict(part="snippet", type="video", q=query, maxResults=5),
                 summarize=search_items)
    call("search.page2", "search",
         dict(part="snippet", type="video", q=query, maxResults=5,
              pageToken=p1["nextPageToken"]), summarize=search_items)
    call("search.channel_order_date", "search",
         dict(part="snippet", type="video", channelId=channel_id, order="date", maxResults=5),
         summarize=search_items)
    _, filt = call("search.duration_short_closedcaption", "search",
                   dict(part="snippet", type="video", q="khan academy", maxResults=5,
                        videoDuration="short", videoCaption="closedCaption"),
                   summarize=search_items)
    ids = [i["id"]["videoId"] for i in filt.get("items", [])]
    if ids:
        call("videos.verify_search_filters", "videos",
             dict(part="contentDetails", id=",".join(ids)), summarize=video_items)

    # Boundary target: second-newest upload so both windows also contain neighbours.
    target = up["items"][1]["contentDetails"]
    stamp = target["videoPublishedAt"]
    call("search.publishedAfter_equals_T", "search",
         dict(part="snippet", type="video", channelId=channel_id, maxResults=10,
              publishedAfter=stamp, publishedBefore=shift(stamp, days=120)),
         summarize=lambda p: dict(target_videoId=target["videoId"], T=stamp,
                                  target_included=target["videoId"] in
                                  [i["id"]["videoId"] for i in p.get("items", [])],
                                  items=search_items(p)))
    call("search.publishedBefore_equals_T", "search",
         dict(part="snippet", type="video", channelId=channel_id, maxResults=10,
              publishedAfter=shift(stamp, days=-120), publishedBefore=stamp),
         summarize=lambda p: dict(target_videoId=target["videoId"], T=stamp,
                                  target_included=target["videoId"] in
                                  [i["id"]["videoId"] for i in p.get("items", [])],
                                  items=search_items(p)))
    call("search.publishedAfter_date_only", "search",
         dict(part="snippet", type="video", q=query, maxResults=1, publishedAfter="2026-10-01"),
         summarize=search_items)
    call("search.empty_result", "search",
         dict(part="snippet", type="video", maxResults=5,
              q="\"zqxjvkwplmrtbgh qqzzxxjjvvkkww\""), summarize=search_items)

    call("auth.invalid_key", "videos", dict(part="id", id=SAMPLES[0]),
         key="AIza" + "0" * 35)


if __name__ == "__main__":
    result = subprocess.run(["security", "find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"],
                            capture_output=True, text=True)
    if result.returncode != 0:
        sys.exit("keychain item not found: " + KEYCHAIN_SERVICE)
    KEY = result.stdout.strip()
    started = now()
    try:
        main()
    finally:
        OUT.write_text(json.dumps(dict(
            started_at=started, completed_at=now(),
            policy=("Read-only calls with a user-owned standard API key restricted to "
                    "YouTube Data API v3; key sent as X-Goog-Api-Key header and never "
                    "recorded; project numbers redacted; third-party comment text and "
                    "authors omitted (lengths and counts only)."),
            transport="python urllib via local HTTP proxy",
            documented_cost_sum=spent, requests=records),
            ensure_ascii=False, indent=2) + "\n")
