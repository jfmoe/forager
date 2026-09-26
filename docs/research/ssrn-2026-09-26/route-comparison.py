"""Compares the ssrn_crossref and ssrn_browser routes on one query set.

Runs each query on each route serially through the forager binary, with the SSRN order pinned
to one route, and writes the raw observations to route-comparison.json next to this file.

Usage: python3 route-comparison.py FORAGER_BINARY
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path

LIMIT = 20
ROUTES = ["ssrn_crossref", "ssrn_browser"]

TOPICAL = [
    "dual momentum",
    "carbon tax",
    "merger arbitrage",
    "ESG ratings disagreement",
    "climate risk disclosure",
    "private credit",
    "cryptocurrency regulation",
    "central bank digital currency",
    "board gender diversity",
    "insider trading enforcement",
    "antitrust digital platforms",
    "artificial intelligence liability",
    "minimum wage employment",
    "inflation expectations",
    "housing affordability",
    "venture capital returns",
    "deposit insurance bank runs",
    "stablecoins",
    "securities class actions",
    "multinational tax avoidance",
]

# Queries a user might type for a paper they know, and the SSRN ID of that paper.
KNOWN = [
    ("betting against beta", "2049939"),
    ("gross profitability premium", "1598056"),
    ("quality minus junk", "2312432"),
    ("value and momentum everywhere", "2174501"),
    ("time series momentum", "2089463"),
    ("five-factor asset pricing model", "2287202"),
    ("fact fiction momentum investing", "2435323"),
    ("factor momentum and the momentum factor", "3014521"),
    ("dual momentum risk premia harvesting", "2042750"),
    ("momentum has its moments", "2041429"),
]


def run(binary, route, query):
    environment = dict(os.environ, FORAGER_PLATFORMS__SSRN__ORDER=json.dumps([route]))
    started = time.monotonic()
    completed = subprocess.run(
        [binary, "platform", "ssrn", "search", query, "--limit", str(LIMIT), "--timeout", "150"],
        capture_output=True,
        env=environment,
        check=False,
    )
    elapsed = time.monotonic() - started
    payload = json.loads(completed.stdout or b"{}")
    return {
        "route": route,
        "query": query,
        "exit": completed.returncode,
        "seconds": round(elapsed, 2),
        "error_kind": payload.get("error_kind"),
        "message": payload.get("message"),
        "items": [
            {
                "ref": item["ref"],
                "depth": item["depth"],
                "title": item["title"],
                "authors": len(item["authors"]),
                "published": item["published"],
                "posted": item["posted"],
                "has_abstract": item["abstract"] is not None,
                "has_snippet": item["snippet"] is not None,
            }
            for item in payload.get("items", [])
        ],
    }


def main():
    binary = sys.argv[1]
    queries = [(query, None) for query in TOPICAL] + KNOWN
    runs = []
    for query, _ in queries:
        for route in ROUTES:
            result = run(binary, route, query)
            runs.append(result)
            print(route, query, result["exit"], result["seconds"], len(result["items"]), flush=True)
            if route == "ssrn_browser" and result["error_kind"] == "auth":
                print("stopped: the browser route reported auth", flush=True)
                break
        else:
            continue
        break
    output = {
        "checked_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "limit": LIMIT,
        "known": dict(KNOWN),
        "runs": runs,
    }
    target = Path(__file__).with_name("route-comparison.json")
    target.write_text(json.dumps(output, ensure_ascii=False, indent=1) + "\n")


if __name__ == "__main__":
    main()
