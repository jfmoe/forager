import json, sys
from pathlib import Path
for path in sys.argv[1:]:
    r = json.loads(Path(path).read_text())
    resp = r["response"]
    print("==", r["label"], r["seconds"], "s")
    for item in resp["output"]:
        if item["type"] != "message":
            print("  call", item["type"], item.get("name"), item.get("input"), item.get("status"))
            continue
        for c in item["content"]:
            anns = [a["url"] for a in c.get("annotations", [])]
            print("  annotations", anns)
            try:
                posts = json.loads(c["text"])["posts"]
            except Exception as e:
                print("  text not JSON", e); continue
            for p in posts:
                print("  post", p["role"], p["url"], repr(p["text"][:60]))
    u = resp.get("usage", {})
    print("  usage sources", u.get("num_sources_used"), "tools", u.get("num_server_side_tools_used"), u.get("server_side_tool_usage_details"))
