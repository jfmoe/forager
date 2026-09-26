import json
import os
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path

from bench import CASES, ENC, ROOT, credentials, save

for sid in ['python', 'quotes_delay']:
    _, kind, url = next(c for c in CASES if c[0] == sid)
    for provider in ['tavily', 'firecrawl']:
        name = f'{sid}__{provider}__cli'
        if (ROOT/(name+'.row.json')).exists():
            continue
        keys = credentials()
        env = os.environ.copy()
        env['FORAGER_CAPABILITIES__WEB_FETCH__ORDER'] = json.dumps([provider])
        env['FORAGER_RETRY__MAX_ATTEMPTS'] = '1'
        env['FORAGER_PROVIDERS__'+provider.upper()+'__KEYS'] = json.dumps([keys[provider]])
        start = time.monotonic()
        process = subprocess.run(['forager','fetch',url,'--format','json','--timeout','120'],env=env,capture_output=True,text=True)
        for key in keys.values():
            assert key not in process.stdout and key not in process.stderr
        save(name+'.json',process.stdout)
        save(name+'.stderr',process.stderr)
        try:
            data = json.loads(process.stdout)
        except ValueError:
            data = {}
        content = data.get('content','')
        save(name+'.md',content)
        row = dict(id=sid,kind=kind,url=url,provider=provider,mode=provider,suffix='cli',file=name,
                   utc=datetime.now(timezone.utc).isoformat(),exit=process.returncode,chars=len(content),
                   tokens=len(ENC.encode(content)),seconds=round(time.monotonic()-start,3))
        save(name+'.row.json',row)
        print(json.dumps(row),flush=True)
        time.sleep(1)
