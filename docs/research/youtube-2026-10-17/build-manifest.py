"""Index the new research artifacts and verify references without changing other files."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent
REPORT = ROOT.parent / "2026-10-17-youtube-integration.md"
BASELINE = "b3e0d456bfa8f7310e3e88cf22b1a49069f6e0e8"


def timestamp(path):
    return datetime.fromtimestamp(path.stat().st_mtime, timezone.utc).isoformat()


def provenance(path):
    if path.suffix == ".txt":
        paired = path.with_suffix(".json")
        if paired.exists():
            return provenance(paired)
    if path.name.startswith("quota-fetch.raw"):
        return ["https://developers.google.com/youtube/v3/determine_quota_cost"], timestamp(path)
    if path.suffix == ".vtt":
        video = "Ks-_Mh1QhMc" if "Ks-_Mh1QhMc" in path.name else "jNQXAC9IVRw"
        record = json.loads((ROOT / ("ytdlp-subtitles-" + video + ".json")).read_text())
        return [record["source_url"]], record["checked_at"]
    if path.suffix == ".json":
        data = json.loads(path.read_text())
        if isinstance(data, list):
            return [item[1] for item in data], timestamp(path)
        urls = []
        if data.get("source_url"):
            urls.append(data["source_url"])
        for item in data.get("files", []):
            if isinstance(item, dict) and item.get("source_url"):
                urls.append(item["source_url"])
        if path.name == "environment.json":
            urls += ["https://www.cloudflare.com/cdn-cgi/trace", "https://github.com/jfmoe/forager",
                     "https://github.com/jackwener/opencli", "https://github.com/yt-dlp/yt-dlp",
                     "https://github.com/cli/cli"]
        if path.name == "credential-availability.json":
            urls += ["file:///Users/jfmoe/.config/forager/config.toml", "process://environment"]
        if not urls:
            urls = [path.as_uri()]
        return list(dict.fromkeys(urls)), data.get("checked_at", timestamp(path))
    return [path.as_uri()], timestamp(path)


content = REPORT.read_text()
definitions = dict(re.findall(r"^\[([^\]]+)\]:\s*(\S+)", content, re.M))
used = set(re.findall(r"\]\[([^\]]+)\]", content))
missing = sorted(used - definitions.keys())
assert not missing, missing
for key, target in definitions.items():
    if not target.startswith(("https://", "http://")):
        assert (REPORT.parent / target).exists() or key == "E-manifest", (key, target)

files = []
for path in sorted(ROOT.iterdir()):
    if not path.is_file() or path.name == "source-manifest.json":
        continue
    urls, checked = provenance(path)
    role = "source_snapshot_or_read_only_observation"
    if path.suffix == ".py":
        role = "task_authored_collection_or_validation_code; not downloaded source"
    if path.suffix == ".vtt":
        role = "first_party_caption_track; downloaded without media"
    if path.name.startswith("document-sources"):
        role = "task_authored_document_retrieval_inputs"
    files.append({"file": path.name, "source_url": urls[0], "source_urls": urls,
                  "retrieved_at": checked, "bytes": path.stat().st_size,
                  "sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "role": role})

manifest = {"checked_on": "2026-10-17", "generated_at": datetime.now(timezone.utc).isoformat(),
            "repository_baseline": BASELINE,
            "hash_algorithm": "sha256",
            "self_exclusion": "This manifest excludes its own bytes to avoid a circular hash; every other file in this directory is indexed.",
            "normalization": "Paired .txt files are readable projections. Direct quota/search origin HTML masks public embedded client keys; each JSON records the pre-redaction body hash. User credential values were never copied.",
            "source_policy": "Primary documentation, terms, source code and first-party read-only API observations. Search output and issue user reports are discovery/report evidence, not independent platform facts.",
            "test_scope": {"youtube_content_samples": ["jNQXAC9IVRw", "Ks-_Mh1QhMc"],
                           "paid_api_tests": 0, "official_keyed_youtube_api_calls": 0,
                           "media_downloads": 0, "downloaded_source_execution": False,
                           "credential_values_recorded": False,
                           "notes": "Document retrieval used existing forager configuration; no purchase, paid provider acceptance request, account/project creation or login performed. Initial yt-dlp dump probes were simulation only; later no-simulate/skip-download probes retrieved subtitle files."},
            "report": {"file": "../" + REPORT.name, "bytes": REPORT.stat().st_size,
                       "sha256": hashlib.sha256(REPORT.read_bytes()).hexdigest(),
                       "source_urls": ["https://github.com/jfmoe/forager/tree/" + BASELINE],
                       "role": "task_authored_synthesis_and_proposal; citations point to indexed primary evidence"},
            "files": files}
(ROOT / "source-manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
print(json.dumps({"files_indexed": len(files), "report_bytes": REPORT.stat().st_size,
                  "reference_definitions": len(definitions), "missing_references": missing}))
