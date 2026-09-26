"""Assess source anchors and marker overhead in saved benchmark responses."""

import hashlib
import html
import json
import os
import re
import unicodedata
from pathlib import Path

import pymupdf
import tiktoken


HERE = Path(__file__).resolve().parent
RAW = Path(os.environ.get(
    "PDF_PARSERS_BENCH_DIR",
    Path.home() / ".local/share/forager/research/firecrawl-pdf-parsers-20260926",
))
SUMMARY = Path(os.environ.get("PDF_PARSERS_SUMMARY_DIR", HERE))
LEGACY = Path.home() / ".local/share/forager/research/firecrawl-tavily-20260926"
ENCODING = tiktoken.get_encoding("cl100k_base")
PAGES = {
    "llama3": [1, 3, 47, 92],
    "bitcoin": [1, 3, 5, 9],
    "irs583": [1, 3, 15, 27],
    "mixtral": [1, 3, 7, 13],
}
SCAN_ANCHORS = [
    "The LinnSequencer",
    "32 Track MIDI Sequence Recorder",
    "Each of the 100 sequences contains 32 simultaneous, polyphonic tracks",
    "Recording a Sequence",
    "Creating a Song",
    "Composition Without Compromise",
    "Additional Features",
    "18720 Oxnard Street, Tarzana, CA 91356",
    "(818) 708-8131 TELEX #298949 LINN UR",
]
MARKER = re.compile(r"<!-- page \d+ -->")


def norm(value):
    value = re.sub(r"!?\[([^]]*)\]\([^)]*\)", r"\1", value)
    value = html.unescape(value)
    return "".join(character for character in unicodedata.normalize("NFKC", value).lower() if character.isalnum())


def probes(name):
    if name == "scan":
        return [{"page": 1, "text": value} for value in SCAN_ANCHORS]
    document = pymupdf.open(RAW / f"{name}.source.pdf")
    selected = []
    for page_number in PAGES[name]:
        lines = [line.strip() for line in document[page_number - 1].get_text().splitlines()]
        lines = [line for line in lines if len(norm(line)) >= 50]
        if not lines:
            continue
        for line in (lines[0], lines[-1]):
            selected.append({"page": page_number, "text": line[:90]})
    return selected


def assess():
    results = []
    references = {name: probes(name) for name in (list(PAGES) + ["scan"])}
    for row_path in sorted(RAW.glob("*__*.row.json")):
        row = json.loads(row_path.read_text())
        name, mode = row["id"], row["mode"]
        path = RAW / f"{name}__{mode}.md"
        origin = "resumed_benchmark" if row.get("round") == 2 else "first_benchmark"
        if name == "mixtral":
            suffix = "default_fresh" if mode == "default" else "pageMarkers_only_fresh"
            legacy_path = LEGACY / f"pdf_paper__firecrawl__{suffix}.md"
            if legacy_path.exists():
                path = legacy_path
                origin = "prior_benchmark"
        markdown = path.read_text() if path.exists() else ""
        body = norm(markdown)
        anchors = references.get(name, [])
        checked = [
            {"page": probe["page"], "text": probe["text"], "found": norm(probe["text"]) in body}
            for probe in anchors
        ]
        marker_free = MARKER.sub("", markdown)
        result = {
            "id": name,
            "mode": mode,
            "origin": origin,
            "success": origin == "prior_benchmark" or (row.get("success") is True and row.get("http") == 200),
            "whitespace_words": len(markdown.split()),
            "markdown_tables": len(re.findall(r"(?m)^\|.*\|\s*$", markdown)),
            "anchor_found": sum(item["found"] for item in checked),
            "anchor_total": len(checked),
            "anchors": checked,
            "mixral_typo": len(re.findall(r"\bMixral\b", markdown, re.I)),
            "marker_count": len(MARKER.findall(markdown)),
            "marker_token_delta": len(ENCODING.encode(markdown, disallowed_special=()))
            - len(ENCODING.encode(marker_free, disallowed_special=())),
            "marker_free_sha256": hashlib.sha256(marker_free.encode()).hexdigest(),
        }
        results.append(result)
    SUMMARY.mkdir(parents=True, exist_ok=True)
    (SUMMARY / "assessments.json").write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n")
    for row in results:
        print(f"{row['id']} {row['mode']}: {row['anchor_found']}/{row['anchor_total']} anchors, "
              f"{row['marker_count']} markers, {row['marker_token_delta']} marker tokens")


if __name__ == "__main__":
    assess()
