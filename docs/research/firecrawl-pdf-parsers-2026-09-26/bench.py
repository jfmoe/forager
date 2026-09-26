"""Run a bounded Firecrawl PDF parser comparison."""

import argparse
import hashlib
import json
import os
import re
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

import pymupdf
import requests
import tiktoken


HERE = Path(__file__).resolve().parent
RAW = Path(os.environ.get(
    "PDF_PARSERS_BENCH_DIR",
    Path.home() / ".local/share/forager/research/firecrawl-pdf-parsers-20260926",
))
RAW.mkdir(parents=True, exist_ok=True)
SUMMARY = Path(os.environ.get("PDF_PARSERS_SUMMARY_DIR", HERE))
SUMMARY.mkdir(parents=True, exist_ok=True)
LEDGER = RAW / "ledger.jsonl"
ENCODING = tiktoken.get_encoding("cl100k_base")
PARSER = {"type": "pdf", "mode": "auto", "pageMarkers": True}
CASES = [
    ("mixtral", "pdf", "https://arxiv.org/pdf/2401.04088v1"),
    ("scan", "pdf", "https://raw.githubusercontent.com/ocrmypdf/OCRmyPDF/main/tests/resources/ccitt.pdf"),
    ("bitcoin", "pdf", "https://bitcoin.org/bitcoin.pdf"),
    ("irs583", "pdf", "https://www.irs.gov/pub/irs-pdf/p583.pdf"),
    ("python", "html", "https://docs.python.org/3/tutorial/datastructures.html"),
    ("mixtral_html", "html", "https://arxiv.org/html/2401.04088v1"),
    ("llama3", "pdf", "https://arxiv.org/pdf/2407.21783"),
]
TARGETS = [
    ("llama3", "markers"),
    ("bitcoin", "default"),
    ("bitcoin", "markers"),
    ("scan", "default"),
    ("scan", "markers"),
    ("irs583", "default"),
    ("irs583", "markers"),
    ("python", "default"),
    ("python", "markers"),
    ("mixtral_html", "default"),
    ("mixtral_html", "markers"),
]
MAX_NEW_CALLS = 16
MAX_NEW_CREDITS = 220


def save_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def key():
    config = tomllib.loads((Path.home() / ".config/forager/config.toml").read_text())
    return config["providers"]["firecrawl"]["keys"][1]


def source():
    source_rows = []
    for name, kind, url in CASES:
        if kind != "pdf":
            continue
        path = RAW / f"{name}.source.pdf"
        if not path.exists():
            response = requests.get(url, timeout=90)
            response.raise_for_status()
            if not response.content.startswith(b"%PDF"):
                raise ValueError(f"{name}: expected a PDF")
            path.write_bytes(response.content)
        data = path.read_bytes()
        document = pymupdf.open(stream=data, filetype="pdf")
        page_text = [page.get_text() for page in document]
        (RAW / f"{name}.truth.txt").write_text("\n\n".join(page_text))
        row = {
            "id": name,
            "url": url,
            "pages": len(document),
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
            "text_chars_per_page": [len(value) for value in page_text],
        }
        source_rows.append(row)
        print(json.dumps({k: row[k] for k in ("id", "pages", "bytes")}), flush=True)
    save_json(SUMMARY / "sources.json", source_rows)


def run():
    api_key = key()
    existing = [json.loads(line) for line in LEDGER.read_text().splitlines()] if LEDGER.exists() else []
    attempts = RAW / "attempts"
    attempts.mkdir(exist_ok=True)
    new_calls = sum(row.get("round") == 2 for row in existing)
    new_credits = sum(
        (json.loads(path.read_text()).get("credits_used") or 0)
        for path in attempts.glob("*.row.json")
    )
    cases = {name: (kind, url) for name, kind, url in CASES}
    last_finished = None
    for name, mode in TARGETS:
        kind, url = cases[name]
        current = RAW / f"{name}__{mode}.row.json"
        if current.exists():
            prior = json.loads(current.read_text())
            if prior.get("http") == 200 and prior.get("success") is True:
                continue
        previous_attempts = [
            row for row in existing
            if row.get("round") == 2 and row["id"] == name and row["mode"] == mode
        ]
        if previous_attempts and current.exists():
            prior = json.loads(current.read_text())
            if (prior.get("http") != 429 and "client_error" not in prior) or len(previous_attempts) >= 2:
                continue
        for retry in range(2):
            if previous_attempts and retry == 0:
                continue
            if new_calls >= MAX_NEW_CALLS or new_credits > MAX_NEW_CREDITS:
                print("Stopped at the request or credit limit.", flush=True)
                return
            if last_finished is None and new_calls:
                prior_rows = sorted(attempts.glob("*.row.json"))
                if prior_rows:
                    latest = json.loads(prior_rows[-1].read_text())
                    finished_at = datetime.fromisoformat(latest["utc"]).timestamp() + latest["seconds"]
                    gap = 60 if retry and latest.get("http") == 429 else 7
                    time.sleep(max(0, gap - (time.time() - finished_at)))
            elif last_finished is not None:
                time.sleep(60 if retry else 7)
            body = {
                "url": url,
                "formats": ["markdown"],
                "onlyMainContent": True,
                "timeout": 60000,
                "maxAge": 0,
            }
            if mode == "markers":
                body["parsers"] = [PARSER]
            row = {
                "call": len(existing) + 1,
                "round": 2,
                "id": name,
                "kind": kind,
                "mode": mode,
                "utc": datetime.now(timezone.utc).isoformat(),
                "request": body,
            }
            with LEDGER.open("a") as output:
                output.write(json.dumps(row) + "\n")
            existing.append(row)
            new_calls += 1
            started = time.monotonic()
            try:
                response = requests.post(
                    "https://api.firecrawl.dev/v2/scrape",
                    headers={"Authorization": f"Bearer {api_key}"},
                    json=body,
                    timeout=125,
                )
                safe_text = response.text.replace(api_key, "(credential omitted)")
                (RAW / f"{name}__{mode}.response.json").write_text(safe_text)
                payload = json.loads(safe_text)
                data = payload.get("data") or {}
                markdown = data.get("markdown") or ""
                (RAW / f"{name}__{mode}.md").write_text(markdown)
                metadata = data.get("metadata") or {}
                row.update({
                    "http": response.status_code,
                    "success": payload.get("success"),
                    "error": payload.get("error"),
                    "metadata_status": metadata.get("statusCode"),
                    "credits_used": metadata.get("creditsUsed"),
                    "num_pages": metadata.get("numPages"),
                    "total_pages": metadata.get("totalPages"),
                    "chars": len(markdown),
                    "words": len(re.findall(r"\b[\w]+(?:[’'-][\w]+)*\b", markdown)),
                    "tokens": len(ENCODING.encode(markdown, disallowed_special=())),
                    "sha256": hashlib.sha256(markdown.encode()).hexdigest(),
                    "page_markers": re.findall(r"<!-- page (\d+) -->", markdown),
                    "markdown_tables": len(re.findall(r"(?m)^\|.*\|\s*$", markdown)),
                })
            except Exception as error:
                row["client_error"] = str(error).replace(api_key, "(credential omitted)")
            row["seconds"] = round(time.monotonic() - started, 3)
            last_finished = time.monotonic()
            save_json(RAW / f"{name}__{mode}.row.json", row)
            save_json(attempts / f"{row['call']:03d}.row.json", row)
            print(json.dumps({field: row.get(field) for field in (
                "call", "id", "mode", "http", "success", "seconds", "credits_used", "words", "client_error"
            )}), flush=True)
            new_credits += row.get("credits_used") or 0
            if row.get("http") == 402:
                print("Stopped after insufficient credits.", flush=True)
                return
            if new_credits > MAX_NEW_CREDITS:
                print("Stopped at the credit limit.", flush=True)
                return
            if row.get("http") != 429:
                break
            if retry == 1:
                print("Rate limit persisted after one retry.", flush=True)
                return


def summarize():
    rows = [json.loads(path.read_text()) for path in sorted(RAW.glob("*__*.row.json"))]
    prior_results = HERE.parent / "firecrawl-tavily-2026-09-26/results.json"
    prior_raw = Path.home() / ".local/share/forager/research/firecrawl-tavily-20260926"
    if prior_results.exists():
        suffixes = {"default_fresh": "default", "pageMarkers_only_fresh": "markers"}
        for prior in json.loads(prior_results.read_text()):
            suffix = prior.get("suffix")
            if prior.get("id") != "pdf_paper" or suffix not in suffixes:
                continue
            markdown_path = prior_raw / f"{prior['file']}.md"
            if not markdown_path.exists():
                continue
            markdown = markdown_path.read_text()
            metadata = prior.get("metadata") or {}
            rows = [row for row in rows if (row["id"], row["mode"]) != ("mixtral", suffixes[suffix])]
            rows.append({
                "call": prior["call"],
                "id": "mixtral",
                "kind": "pdf",
                "mode": suffixes[suffix],
                "origin": "prior_benchmark",
                "utc": prior["utc"],
                "request": prior["request"],
                "http": prior["http"],
                "success": prior["http"] == 200,
                "metadata_status": metadata.get("statusCode"),
                "credits_used": prior.get("credits_reported"),
                "num_pages": metadata.get("numPages"),
                "total_pages": metadata.get("totalPages"),
                "chars": len(markdown),
                "words": len(re.findall(r"\b[\w]+(?:[’'-][\w]+)*\b", markdown)),
                "tokens": prior["tokens"],
                "sha256": prior["sha256"],
                "page_markers": re.findall(r"<!-- page (\d+) -->", markdown),
                "markdown_tables": len(re.findall(r"(?m)^\|.*\|\s*$", markdown)),
                "whitespace_words": len(markdown.split()),
                "seconds": prior["seconds"],
            })
    for row in rows:
        if row.get("origin") == "prior_benchmark":
            continue
        row["origin"] = "resumed_benchmark" if row.get("round") == 2 else "first_benchmark"
        markdown = RAW / f"{row['id']}__{row['mode']}.md"
        row["whitespace_words"] = len(markdown.read_text().split()) if markdown.exists() else None
    rows.sort(key=lambda row: (row["id"], row["mode"]))
    save_json(SUMMARY / "results.json", rows)
    print(f"{len(rows)} completed rows; {len(LEDGER.read_text().splitlines())} recorded calls")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["source", "run", "summarize"])
    arguments = parser.parse_args()
    {"source": source, "run": run, "summarize": summarize}[arguments.action]()
