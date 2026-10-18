"""Send the exact request forager's X fetch sends and save the completed response."""
import json, sys, tomllib, urllib.request, time
from pathlib import Path

POST_RULES = "List only posts that your x_search tool calls returned in this response; never list a post from memory, and never invent or alter a URL. For each post give its URL in the form https://x.com/<handle>/status/<post_id>, the author's handle without @, and the full post text exactly as the tool returned it: do not translate, summarize, or shorten it."
THREAD = "You read X (formerly Twitter) with the x_search tool. Fetch the thread of the requested post with the tool's thread fetch, then list the posts it returned in conversation order: the requested post with role root, the posts it replies to with role parent, the replies to it with role reply, and the posts it quotes with role quoted."
METADATA = "You read X (formerly Twitter) with the x_search tool. Fetch the requested post with the tool's thread fetch, then list only that post, with role root."
MISSING = "If the requested post does not exist or is not visible, return an empty posts array."

def body(post_id, depth, model, tool_choice=None):
    task = THREAD if depth == "thread" else METADATA
    item = {"type": "object", "additionalProperties": False,
            "required": ["url", "author_handle", "text", "role"],
            "properties": {"url": {"type": "string"}, "author_handle": {"type": "string"},
                           "text": {"type": "string"},
                           "role": {"type": "string", "enum": ["parent", "root", "reply", "quoted"]}}}
    return {"model": model, "instructions": f"{task} {POST_RULES} {MISSING}",
            "input": [{"role": "user", "content": f"The requested post: ID {post_id}, https://x.com/i/status/{post_id}"}],
            "stream": True, "tools": [{"type": "x_search"}],
            "text": {"format": {"type": "json_schema", "name": "x_thread", "strict": True,
                                "schema": {"type": "object", "additionalProperties": False, "required": ["posts"],
                                           "properties": {"posts": {"type": "array", "items": item}}}}},
            **({"tool_choice": tool_choice} if tool_choice else {})}

def main(config_path, out_dir, post_id, depth, label, tool_choice=None):
    cfg = tomllib.loads(Path(config_path).read_text())["providers"]["xai"]
    req = urllib.request.Request(cfg["url"].rstrip("/") + "/responses",
        data=json.dumps(body(post_id, depth, cfg["model"], tool_choice)).encode(), method="POST",
        headers={"authorization": "Bearer " + cfg["keys"][0], "accept": "text/event-stream",
                 "content-type": "application/json"})
    started = time.time()
    events, completed = [], None
    with urllib.request.urlopen(req, timeout=180) as resp:
        status = resp.status
        for raw in resp:
            line = raw.decode("utf-8", "replace").rstrip("\n")
            if not line.startswith("data:"):
                continue
            data = line[5:].strip()
            if data == "[DONE]":
                continue
            ev = json.loads(data)
            events.append(ev.get("type"))
            if ev.get("type") == "response.completed":
                completed = ev["response"]
    record = {"label": label, "post_id": post_id, "depth": depth, "tool_choice": tool_choice, "model": cfg["model"],
              "endpoint_host": cfg["url"].split("/")[2], "http_status": status,
              "seconds": round(time.time() - started, 1),
              "event_type_counts": {t: events.count(t) for t in dict.fromkeys(events)},
              "response": completed}
    Path(out_dir, f"{label}.json").write_text(json.dumps(record, ensure_ascii=False, indent=2) + "\n")
    print(label, "status", status, "seconds", record["seconds"])

if __name__ == "__main__":
    main(*sys.argv[1:])
