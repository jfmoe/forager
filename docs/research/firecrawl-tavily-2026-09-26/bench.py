import argparse
import hashlib
import json
import os
import re
import subprocess
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

import pymupdf
import requests
import tiktoken
from bs4 import BeautifulSoup

ROOT = Path(os.environ.get('FETCH_BENCH_DIR', Path(__file__).resolve().parent))
ROOT.mkdir(parents=True, exist_ok=True)
ENC = tiktoken.get_encoding('cl100k_base')
CASES = [
    ('mdn', 'ordinary', 'https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API/Using_Fetch'),
    ('python', 'ordinary', 'https://docs.python.org/3/tutorial/datastructures.html'),
    ('essay', 'ordinary', 'https://paulgraham.com/greatwork.html'),
    ('chinese', 'ordinary', 'https://zh.wikipedia.org/wiki/大语言模型'),
    ('chinese_docs', 'ordinary', 'https://developer.mozilla.org/zh-CN/docs/Web/JavaScript/Guide/Introduction'),
    ('quotes_js', 'special', 'https://quotes.toscrape.com/js/'),
    ('quotes_delay', 'special', 'https://quotes.toscrape.com/js-delayed/'),
    ('hn', 'special', 'https://news.ycombinator.com/item?id=8863'),
    ('ssrn', 'special', 'https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750'),
    ('arxiv_html', 'special', 'https://arxiv.org/html/2401.04088v1'),
    ('wiki_table', 'special', 'https://en.wikipedia.org/wiki/List_of_countries_by_population_(United_Nations)'),
    ('pdf_simple', 'pdf', 'https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf'),
    ('pdf_bitcoin', 'pdf', 'https://bitcoin.org/bitcoin.pdf'),
    ('pdf_paper', 'pdf', 'https://arxiv.org/pdf/2401.04088v1'),
    ('pdf_scan', 'pdf', 'https://raw.githubusercontent.com/ocrmypdf/OCRmyPDF/main/tests/resources/ccitt.pdf'),
]


def save(name, value):
    text = value if isinstance(value, str) else json.dumps(value, ensure_ascii=False, indent=2)
    (ROOT / name).write_text(text)


def direct():
    for sid, kind, url in CASES:
        path = ROOT / (sid + '.source')
        if path.exists():
            continue
        start = time.monotonic()
        try:
            r = requests.get(url, timeout=90)
            path.write_bytes(r.content)
            row = dict(id=sid, kind=kind, url=url, final_url=r.url, status=r.status_code,
                       seconds=round(time.monotonic()-start, 3), bytes=len(r.content),
                       content_type=r.headers.get('Content-Type'), sha256=hashlib.sha256(r.content).hexdigest())
            if r.content.startswith(b'%PDF'):
                doc = pymupdf.open(stream=r.content, filetype='pdf')
                row['pages'] = len(doc)
                page_text = [p.get_text() for p in doc]
                row['text_chars_per_page'] = [len(t) for t in page_text]
                save(sid + '.truth.txt', '\n\n'.join(page_text))
                for page in sorted(set([0, min(3, len(doc)-1)])):
                    doc[page].get_pixmap(matrix=pymupdf.Matrix(1.4, 1.4)).save(ROOT / f'{sid}.page{page+1}.png')
            else:
                soup = BeautifulSoup(r.content, 'html.parser')
                for node in soup(['script', 'style', 'noscript']):
                    node.decompose()
                save(sid + '.truth.txt', soup.get_text(' ', strip=True))
            save(sid + '.source.json', row)
            print(json.dumps(row, ensure_ascii=False), flush=True)
        except Exception as exc:
            save(sid + '.source.error.json', {'error': str(exc)})
            print(sid, type(exc).__name__, flush=True)
        time.sleep(3.3 if 'arxiv.org' in url else 0.2)


def credentials():
    config = tomllib.loads(Path.home().joinpath('.config/forager/config.toml').read_text())
    return {p: config['providers'][p]['keys'][0] for p in ['tavily', 'firecrawl']}


def run(sid, mode, suffix='', changes=None):
    _, kind, url = next(c for c in CASES if c[0] == sid)
    name = f'{sid}__{mode}' + ('__' + suffix if suffix else '')
    if (ROOT / (name + '.row.json')).exists():
        return
    provider = 'firecrawl' if mode == 'firecrawl' else 'tavily'
    ledger_path = ROOT / 'ledger.jsonl'
    previous = [json.loads(line) for line in ledger_path.read_text().splitlines()] if ledger_path.exists() else []
    if provider == 'firecrawl':
        source = json.loads((ROOT / (sid + '.source.json')).read_text())
        reserve = source['pages'] if kind == 'pdf' else 6
    else:
        reserve = 0.4 if mode == 'advanced' else 0.2
    keys = credentials()
    def clean(value):
        for key in keys.values():
            value = value.replace(key, '[REMOVED]')
        return value
    if provider == 'firecrawl':
        endpoint = 'https://api.firecrawl.dev/v2/scrape'
        body = {'url':url, 'formats':['markdown'], 'onlyMainContent':True, 'timeout':60000}
    else:
        endpoint = 'https://api.tavily.com/extract'
        body = {'urls':[url], 'format':'markdown', 'extract_depth':mode, 'timeout':60, 'include_usage':True}
    if changes:
        body.update(changes)
    row = dict(call=len(previous)+1, id=sid, kind=kind, provider=provider, mode=mode, suffix=suffix,
               url=url, request=body, file=name, utc=datetime.now(timezone.utc).isoformat(),
               budget_charge=reserve)
    with ledger_path.open('a') as f:
        f.write(json.dumps(row, ensure_ascii=False)+'\n')
    start = time.monotonic()
    content = ''
    try:
        response = requests.post(endpoint, headers={'Authorization':'Bearer '+keys[provider]}, json=body, timeout=150)
        save(name + '.json', clean(response.text))
        row['http'] = response.status_code
        result = response.json()
        if provider == 'firecrawl':
            data = result.get('data') or {}
            content = data.get('markdown') or ''
            row['metadata'] = data.get('metadata')
            row['error'] = result.get('error')
            row['warning'] = data.get('warning')
            credit = (data.get('metadata') or {}).get('creditsUsed')
        else:
            content = next(iter(result.get('results') or []), {}).get('raw_content') or ''
            row['failed_results'] = result.get('failed_results')
            row['usage'] = result.get('usage')
            row['provider_response_time'] = result.get('response_time')
            credit = (result.get('usage') or {}).get('credits')
        if isinstance(credit, (float, int)):
            row['budget_charge'] = credit
            row['credits_reported'] = credit
    except Exception as exc:
        row['error'] = clean(str(exc))
    row['seconds'] = round(time.monotonic()-start, 3)
    content = clean(content)
    save(name + '.md', content)
    row['chars'] = len(content)
    row['tokens'] = len(ENC.encode(content, disallowed_special=()))
    row['sha256'] = hashlib.sha256(content.encode()).hexdigest()
    save(name + '.row.json', row)
    previous.append(row)
    ledger_path.write_text(''.join(json.dumps(r, ensure_ascii=False)+'\n' for r in previous))
    print(json.dumps({k:row.get(k) for k in ['call','id','mode','suffix','http','chars','tokens','seconds','credits_reported','budget_charge','error']}, ensure_ascii=False), flush=True)
    time.sleep(3.3 if 'arxiv.org' in url else 1)


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('action', choices=['direct','base','one'])
    p.add_argument('--id')
    p.add_argument('--mode', choices=['basic','advanced','firecrawl'])
    p.add_argument('--suffix', default='')
    p.add_argument('--changes', default='{}')
    args = p.parse_args()
    save('cases.json', [{'id':sid,'kind':kind,'url':url} for sid,kind,url in CASES])
    if args.action == 'direct':
        direct()
    elif args.action == 'base':
        for index, (sid, _, _) in enumerate(CASES):
            modes = ['basic','advanced','firecrawl']
            modes = modes[index % 3:] + modes[:index % 3]
            for mode in modes:
                run(sid, mode)
    else:
        run(args.id, args.mode, args.suffix, json.loads(args.changes))
