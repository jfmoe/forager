"""Bounded, anonymous transport observations; no retries or challenge solving."""
import datetime
import hashlib
import json
import pathlib
import re
import subprocess
import time

OUT = pathlib.Path(__file__).parent
UAS = {
    "honest": "forager-reddit-integration-research/0.13.0 (local read-only feasibility check; no OAuth; 2026-10-17)",
    "browser": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
}
TARGETS = []
for host in ["www.reddit.com", "old.reddit.com"]:
    TARGETS.extend([
        (host + "-search-json", f"https://{host}/search.json?q=rust&limit=1"),
        (host + "-thread-json", f"https://{host}/comments/1wubcvf.json?limit=1&depth=1"),
        (host + "-search-rss", f"https://{host}/search.rss?q=rust&limit=1"),
    ])
TARGETS.extend([
    ("svc-search", "https://www.reddit.com/svc/shreddit/search/?q=rust&type=posts&t=month&sort=relevance"),
    ("svc-subreddit-search", "https://www.reddit.com/svc/shreddit/r/rust/search/?q=async&type=posts&t=month&sort=relevance"),
    ("svc-listing", "https://www.reddit.com/svc/shreddit/community-more-posts/new/?name=rust"),
    ("svc-comments", "https://www.reddit.com/svc/shreddit/comments/r/redditdev/t3_1wubcvf"),
    ("redd-it", "https://redd.it/1wubcvf"),
])
summary = []
for label, url in TARGETS:
    for ua_name, ua in UAS.items():
        time.sleep(2.2)
        stamp = datetime.datetime.now(datetime.timezone.utc).isoformat()
        cmd = ["curl", "--silent", "--show-error", "--compressed", "--max-time", "25", "--max-redirs", "0", "--user-agent", ua, "--include", "--write-out", "\nFORAGER_META:%{json}", url]
        proc = subprocess.run(cmd, capture_output=True)
        raw, _, meta = proc.stdout.rpartition(b"\nFORAGER_META:")
        try:
            info = json.loads(meta)
        except ValueError:
            info = {}
        text = raw.decode("utf-8", errors="replace")
        headers, _, body = text.partition("\r\n\r\n")
        header_blocks = [headers]
        if re.match(r"^HTTP/\S+ 200 Connection established", headers, re.I):
            headers, _, body = body.partition("\r\n\r\n")
            header_blocks.append(headers)
        lower = body.lower()
        markers = [s for s in ["blocked by network security", "whoa there", "you've been blocked", "your request has been blocked", "js-challenge", "recaptcha", "verify you are human", "too many requests", "blocked due to a network policy"] if s in lower]
        entry = {"checked_at": stamp, "url": url, "ua_kind": ua_name, "user_agent": ua, "exit_code": proc.returncode, "stderr": proc.stderr.decode(errors="replace"), "http": info, "headers": headers, "header_blocks": header_blocks, "body": body, "sha256": hashlib.sha256(raw).hexdigest(), "block_markers": markers, "followed_redirect": False}
        filename = label + "-" + ua_name + ".json"
        (OUT / filename).write_text(json.dumps(entry, ensure_ascii=False, indent=2))
        row = {"label": label, "ua": ua_name, "file": filename, "status": info.get("http_code"), "content_type": info.get("content_type"), "redirect_url": info.get("redirect_url"), "markers": markers, "bytes": len(body.encode())}
        summary.append(row)
        (OUT / "anonymous-summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2))
        print(json.dumps(row), flush=True)
for host in ["www.reddit.com", "old.reddit.com"]:
    time.sleep(2.2)
    url = f"https://{host}/robots.txt"
    proc = subprocess.run(["curl", "--silent", "--show-error", "--max-time", "25", "--include", "--user-agent", UAS["honest"], url], capture_output=True, text=True)
    (OUT / (host + "-robots.json")).write_text(json.dumps({"checked_at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "url": url, "exit_code": proc.returncode, "stdout": proc.stdout, "stderr": proc.stderr}, ensure_ascii=False, indent=2))
    print(host, "robots", proc.returncode, flush=True)
