"""Compare SSRN search routes without fetching papers.

Usage: python3 search-comparison.py FORAGER_BINARY
"""

import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import time


HERE = Path(__file__).resolve().parent
BASELINE = runpy.run_path(str(HERE.parent / "ssrn-2026-09-26/route-comparison.py"))
ROUTES = ["ssrn_crossref", "ssrn_browser"]


def main():
    binary = str(Path(sys.argv[1]).resolve())
    output = {
        "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "binary": binary,
        "limit": 20,
        "browser_session": "persistent",
        "browser_window": "foreground",
        "topical": BASELINE["TOPICAL"],
        "known": dict(BASELINE["KNOWN"]),
        "runs": [],
    }
    target = HERE / "search-comparison.json"
    queries = BASELINE["TOPICAL"] + [query for query, _ in BASELINE["KNOWN"]]
    browser_paused = False
    for index, query in enumerate(queries):
        routes = ROUTES if index % 2 == 0 else list(reversed(ROUTES))
        for route in routes:
            if route == "ssrn_browser" and browser_paused:
                continue
            environment = dict(
                os.environ,
                FORAGER_PLATFORMS__SSRN__ORDER=json.dumps([route]),
                OPENCLI_SITE_SESSION="persistent",
                OPENCLI_WINDOW="foreground",
            )
            started = time.monotonic()
            completed = subprocess.run(
                [binary, "platform", "ssrn", "search", query,
                 "--limit", "20", "--timeout", "120"],
                capture_output=True, env=environment, check=False,
            )
            payload = json.loads(completed.stdout or b"{}")
            result = {
                "query": query,
                "route": route,
                "exit": completed.returncode,
                "seconds": round(time.monotonic() - started, 2),
                "payload": payload,
            }
            output["runs"].append(result)
            if route == "ssrn_browser" and payload.get("error_kind") == "auth":
                browser_paused = True
                output["browser_paused_on_auth"] = query
            output["updated_at_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
            target.write_text(json.dumps(output, ensure_ascii=False, indent=2) + "\n")
            print(json.dumps({
                "query": query, "route": route, "exit": completed.returncode,
                "seconds": result["seconds"], "items": len(payload.get("items", [])),
                "error_kind": payload.get("error_kind"),
            }, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
