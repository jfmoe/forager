"""Summarize paired search results and export titles for a blind review."""

from collections import Counter
import json
from pathlib import Path
import random
import statistics


HERE = Path(__file__).resolve().parent
data = json.loads((HERE / "search-comparison.json").read_text())
routes = ["ssrn_crossref", "ssrn_browser"]
runs = {(run["query"], run["route"]): run for run in data["runs"]}
summary = {"routes": {}, "known": [], "overlap": []}
for route in routes:
    attempts = [run for run in data["runs"] if run["route"] == route]
    success = [run for run in attempts if run["exit"] == 0]
    items = [item for run in success for item in run["payload"]["items"]]
    latency = sorted(run["seconds"] for run in success)
    summary["routes"][route] = {
        "attempts": len(attempts),
        "success": len(success),
        "errors": [run for run in attempts if run["exit"] != 0],
        "items": len(items),
        "fields": {key: sum(bool(item.get(key)) for item in items)
                   for key in ["abstract", "snippet", "authors", "posted", "published"]},
        "depths": dict(Counter(item["depth"] for item in items)),
        "p50_seconds": statistics.median(latency) if latency else None,
        "p95_seconds": latency[max(0, (95 * len(latency) + 99) // 100 - 1)] if latency else None,
        "duplicates_within_pages": sum(
            len(run["payload"]["items"]) - len({item["ref"] for item in run["payload"]["items"]})
            for run in success),
    }
for query, target in data["known"].items():
    row = {"query": query, "target": target}
    for route in routes:
        run = runs.get((query, route))
        row[route] = next((index + 1 for index, item in enumerate(run["payload"].get("items", []))
                           if item["ref"] == "ssrn:" + target), None) if run else None
    summary["known"].append(row)
for query in data["topical"] + list(data["known"]):
    pair = [runs.get((query, route)) for route in routes]
    if not all(run and run["exit"] == 0 for run in pair):
        continue
    refs = [{item["ref"] for item in run["payload"]["items"]} for run in pair]
    summary["overlap"].append({
        "query": query, "common": len(refs[0] & refs[1]),
        "crossref_only": len(refs[0] - refs[1]), "browser_only": len(refs[1] - refs[0]),
        "jaccard": len(refs[0] & refs[1]) / len(refs[0] | refs[1]) if any(refs) else None,
    })
ratings_path = HERE / "title-ratings.json"
if ratings_path.exists():
    ratings = json.loads(ratings_path.read_text())
    scores = {(row["query"], item["title"]): item["score"]
              for row in ratings["queries"] for item in row["ratings"]}
    summary["title_match"] = {"routes": {}, "queries": []}
    for route in routes:
        counts = Counter(scores[(query, item["title"])]
                         for query in data["topical"]
                         for item in runs[(query, route)]["payload"]["items"][:5])
        summary["title_match"]["routes"][route] = dict(sorted(counts.items()))
    for query in data["topical"]:
        row = {"query": query}
        for route in routes:
            row[route] = sum(scores[(query, item["title"])] == 2
                             for item in runs[(query, route)]["payload"]["items"][:5])
        summary["title_match"]["queries"].append(row)
(HERE / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
packet = []
rng = random.Random(928)
for query in data["topical"]:
    titles = {}
    for route in routes:
        for item in runs.get((query, route), {}).get("payload", {}).get("items", [])[:5]:
            titles[item["title"]] = True
    entries = list(titles)
    rng.shuffle(entries)
    packet.append({"query": query, "titles": entries})
(HERE / "title-review-input.json").write_text(json.dumps(packet, ensure_ascii=False, indent=2) + "\n")
print(json.dumps(summary, ensure_ascii=False, indent=2))
