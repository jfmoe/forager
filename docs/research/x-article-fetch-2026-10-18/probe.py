"""Fetch one URL through Tavily, Jina, or Firecrawl with one request variant and save the reply."""
import json, sys, time, tomllib, urllib.request, urllib.error
from pathlib import Path

def call(req):
    started = time.time()
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            return resp.status, resp.read().decode("utf-8", "replace"), round(time.time() - started, 1)
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace"), round(time.time() - started, 1)

def main(config_path, out_dir, provider, variant, url, label):
    cfg = tomllib.loads(Path(config_path).read_text())["providers"][provider]
    key = cfg["keys"][0]
    if provider == "tavily":
        body = {"urls": [url], "format": "markdown", "extract_depth": variant}
        req = urllib.request.Request("https://api.tavily.com/extract", data=json.dumps(body).encode(),
            headers={"authorization": f"Bearer {key}", "content-type": "application/json"}, method="POST")
    elif provider == "jina":
        headers = {"authorization": f"Bearer {key}", "x-return-format": "markdown", "accept": "application/json"}
        if variant != "default":
            for pair in variant.split(","):
                name, value = pair.split("=", 1)
                headers[name] = value
        req = urllib.request.Request(f"https://r.jina.ai/{url}", headers=headers)
    else:
        body = {"url": url, "formats": ["markdown"], "onlyMainContent": variant == "main", "timeout": 60000}
        req = urllib.request.Request("https://api.firecrawl.dev/v2/scrape", data=json.dumps(body).encode(),
            headers={"authorization": f"Bearer {key}", "content-type": "application/json"}, method="POST")
    status, text, seconds = call(req)
    try:
        payload = json.loads(text)
    except json.JSONDecodeError:
        payload = text
    if provider == "tavily" and isinstance(payload, dict) and payload.get("results"):
        content = payload["results"][0].get("raw_content") or ""
    elif provider == "jina" and isinstance(payload, dict):
        content = (payload.get("data") or {}).get("content") or ""
    elif provider == "firecrawl" and isinstance(payload, dict):
        content = (payload.get("data") or {}).get("markdown") or ""
    else:
        content = ""
    record = {"provider": provider, "variant": variant, "url": url, "http_status": status,
              "seconds": seconds, "content_chars": len(content), "content": content,
              "response": payload if not content else {k: v for k, v in payload.items() if k not in ("data", "results")}}
    Path(out_dir, f"{label}.json").write_text(json.dumps(record, ensure_ascii=False, indent=2) + "\n")
    print(f"{label}: HTTP {status}, {seconds}s, {len(content)} chars")
    if not content:
        print("   ", json.dumps(payload, ensure_ascii=False)[:300])

if __name__ == "__main__":
    main(*sys.argv[1:])
