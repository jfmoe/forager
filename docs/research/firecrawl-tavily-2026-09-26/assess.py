import json
import html
import os
import re
import unicodedata
from pathlib import Path

from bs4 import BeautifulSoup

ROOT = Path(os.environ.get('FETCH_BENCH_DIR', Path(__file__).resolve().parent))


def norm(text):
    text = re.sub(r'!?\[([^\]]*)\]\((?:[^()]|\([^()]*\))*\)', r'\1', text)
    text = re.sub(r'<br\s*/?>', ' ', text, flags=re.I)
    text = html.unescape(text)
    return ''.join(c for c in unicodedata.normalize('NFKC', text).lower() if c.isalnum())


def source(sid):
    path = ROOT / (sid + '.browser.html')
    if not path.exists():
        path = ROOT / (sid + '.source')
    soup = BeautifulSoup(path.read_bytes(), 'html.parser')
    if sid in ['mdn', 'chinese_docs']:
        scope = soup.select_one('main')
    elif sid == 'python':
        scope = soup.select_one('[role="main"]')
    elif sid == 'arxiv_html':
        scope = soup.select_one('article')
    elif sid in ['chinese', 'wiki_table']:
        scope = soup.select_one('#mw-content-text > .mw-parser-output')
    elif sid == 'essay':
        scope = soup.select_one('font[size="2"]')
    else:
        scope = soup
    assert scope is not None, sid
    return scope


def probes():
    all_probes = {}
    for sid in ['mdn', 'python', 'chinese_docs', 'arxiv_html', 'chinese']:
        scope = source(sid)
        paragraphs = [norm(p.get_text(' ', strip=True)) for p in scope.select('p')]
        paragraphs = [p for p in paragraphs if len(p) >= 80]
        heads = [norm(p.get_text(' ', strip=True).replace('¶','')) for p in scope.select('h1,h2,h3,h4')]
        code = [norm(p.get_text()) for p in scope.select('pre')]
        all_probes[sid] = {
            'paragraph_edges': [v for p in paragraphs for v in [p[:60], p[-60:]]],
            'headings': heads,
            'code_edges': [v for c in code if len(c) >= 50 for v in [c[:50], c[-50:]]],
        }
    all_probes['python']['method_names'] = ['list'+x for x in ['append','extend','insert','remove','pop','clear','index','count','sort','reverse','copy']]
    text = norm(source('essay').get_text(' ', strip=True))
    all_probes['essay'] = {'distributed_text': [text[int(i*(len(text)-100)/39):int(i*(len(text)-100)/39)+100] for i in range(40)]}
    comments = [norm(p.get_text(' ',strip=True)) for p in source('hn').select('.commtext')]
    all_probes['hn'] = {'comments': [p[:min(60,len(p))] for p in comments]}
    quote_data = json.loads(json.loads((ROOT/'quotes_delay.browser.json').read_text()))
    for sid in ['quotes_js', 'quotes_delay']:
        all_probes[sid] = {'quotes': [norm(q['text']) for q in quote_data], 'authors':[norm(q['author']) for q in quote_data]}
    scope = source('arxiv_html')
    all_probes['arxiv_html']['captions'] = [norm(p.get_text(' ',strip=True))[:80] for p in scope.select('figcaption')]
    all_probes['arxiv_html']['references'] = [norm(p.get_text(' ',strip=True))[:60] for p in scope.select('li.ltx_bibitem')]
    rows = source('wiki_table').select('table.wikitable tr')
    all_probes['wiki_table'] = {'population_pairs': [norm(' '.join(c.get_text(' ',strip=True) for c in row.select('td')[1:3])) for row in rows if len(row.select('td')) >= 3]}
    all_probes['pdf_simple'] = {'text':[norm('Dummy PDF file')]}
    for sid in ['pdf_bitcoin','pdf_paper']:
        raw = (ROOT/(sid+'.truth.txt')).read_text()
        lines = [norm(line) for line in raw.splitlines() if len(norm(line)) >= 50]
        picks = [lines[int(i*(len(lines)-1)/29)] for i in range(30)]
        all_probes[sid] = {'distributed_lines':picks}
    all_probes['pdf_scan'] = {'manual_anchors': [norm(x) for x in [
        'The LinnSequencer', '32 Track MIDI Sequence Recorder',
        'Each of the 100 sequences contains 32 simultaneous, polyphonic tracks',
        'Ultra-fast 3½" disk drive stores complex songs in seconds and holds over 110,000 notes per disk',
        'Recording a Sequence', 'Creating a Song', 'Composition Without Compromise',
        'Additional Features', 'Utilizes ultra high-speed, 8 MHz 80186 16 bit computer internally for FAST operation',
        'TEMPO CHANGES may be programmed in a sequence, with smooth transitions if desired',
        '18720 Oxnard Street, Tarzana, CA 91356', '(818) 708-8131 TELEX #298949 LINN UR'
    ]]}
    all_probes['ssrn'] = {'metadata':[norm(x) for x in ['Risk Premia Harvesting Through Dual Momentum','Gary Antonacci','2042750']]}
    return all_probes


if __name__ == '__main__':
    truth = probes()
    (ROOT/'probes.json').write_text(json.dumps(truth,ensure_ascii=False,indent=2))
    results = []
    for path in sorted(ROOT.glob('*.row.json')):
        row = json.loads(path.read_text())
        body = norm((ROOT/(row['file']+'.md')).read_text())
        scores = {}
        for category, probes_ in truth.get(row['id'],{}).items():
            probes_ = [p for p in probes_ if p]
            matched = [p for p in probes_ if p in body]
            scores[category] = {'found':len(matched),'total':len(probes_),'missing':[p for p in probes_ if p not in body]}
        row['scores'] = scores
        results.append(row)
        print(row['file'], ' | '.join(f'{k}:{v["found"]}/{v["total"]}' for k,v in scores.items()))
    (ROOT/'assessments.json').write_text(json.dumps(results,ensure_ascii=False,indent=2))
