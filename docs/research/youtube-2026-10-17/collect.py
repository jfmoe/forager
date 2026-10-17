"""Bounded, read-only evidence collection; run with python3 -I."""
import concurrent.futures
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def write(name, value):
    (ROOT / name).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def run(name, command, source_url, timeout=180):
    started = now()
    result = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    record = dict(source_url=source_url, checked_at=started, completed_at=now(),
                  command=command, exit_code=result.returncode,
                  stdout=result.stdout, stderr=result.stderr)
    write(name + ".json", record)
    return record


def fetch(pair):
    name, url = pair
    record = run(name, ["forager", "fetch", url, "--format", "json"], url)
    try:
        payload = json.loads(record["stdout"])
        text = payload.get("content", "")
        (ROOT / (name + ".txt")).write_text(text)
        print(name, record["exit_code"], len(text), flush=True)
    except (ValueError, AttributeError):
        print(name, record["exit_code"], "no content", flush=True)


if __name__ == "__main__":
    jobs = json.loads((ROOT / sys.argv[1]).read_text())
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        list(pool.map(fetch, jobs))
