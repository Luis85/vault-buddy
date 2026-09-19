"""Run all eight suites against one unchanged HTML; JSON failures fail the run too.

Requires Python Playwright, Chromium at /usr/bin/chromium, ffmpeg and ffprobe.
Origin-backed storage checks report BLOCKED in set_content environments; these are
not counted as passes. Fresh-context project-file round trips run separately.
"""
from pathlib import Path
import collections
import hashlib
import json
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
HTML = ROOT / 'vault-buddy-editor.html'
(ROOT / 'screenshots').mkdir(exist_ok=True)
(ROOT / 'tests/artifacts').mkdir(exist_ok=True)
SUITES = [
    ('focus-ui.py', 'focus-results.json'),
    ('quality.py', 'quality-results.json'),
    ('onboarding.py', 'onboarding-results.json'),
    ('editing.py', 'editing-results.json'),
    ('regression.py', 'results.json'),
    ('webcam-smoke.py', 'webcam-results.json'),
    ('render-review.py', 'render-review-results.json'),
    ('handover.py', 'handover-results.json'),
]
sha = hashlib.sha256(HTML.read_bytes()).hexdigest()
summary = {'artifact_sha256': sha, 'suites': [], 'totals': {}}
for name, result_name in SUITES:
    print('RUN', name, flush=True)
    start = time.monotonic()
    result_file = ROOT / 'tests' / result_name
    result_file.unlink(missing_ok=True)  # A crashed suite must not reuse stale evidence.
    record = {'suite': name, 'result_file': result_name}
    try:
        with (ROOT / 'tests' / ('suite-' + name + '.log')).open('w') as log:
            process = subprocess.run([sys.executable, str(ROOT / 'tests' / name)],
                                     cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,
                                     timeout=300, check=False)
        record['exit_code'] = process.returncode
        data = json.loads(result_file.read_text())
        rows = data if isinstance(data, list) else data.get('results', data.get('scenarios'))
        if not isinstance(rows, list) or not rows:
            raise ValueError('Suite did not report scenario results')
        statuses = [row.get('status', row.get('result')) for row in rows]
        if any(status not in {'PASS', 'FAIL', 'BLOCKED'} for status in statuses):
            raise ValueError(f'Unexpected scenario status: {statuses}')
        counts = collections.Counter(statuses)
        record['counts'] = dict(counts)
        record['failures'] = [row for row in rows if row.get('status', row.get('result')) == 'FAIL']
        record['blocked'] = [row for row in rows if row.get('status', row.get('result')) == 'BLOCKED']
        record['successful'] = process.returncode == 0 and not counts['FAIL']
    except (subprocess.TimeoutExpired, OSError, ValueError, TypeError) as error:
        record.update(successful=False, error=str(error))
    record['seconds'] = round(time.monotonic() - start, 1)
    record['artifact_unchanged'] = sha == hashlib.sha256(HTML.read_bytes()).hexdigest()
    record['successful'] = record.get('successful', False) and record['artifact_unchanged']
    summary['suites'].append(record)
    total = collections.Counter()
    for suite in summary['suites']:
        total.update(suite.get('counts', {}))
    summary['totals'] = dict(total)
    (ROOT / 'tests/final-suite-results.json').write_text(json.dumps(summary, indent=2))
    print('DONE', record, flush=True)
summary['successful'] = all(suite['successful'] for suite in summary['suites'])
(ROOT / 'tests/final-suite-results.json').write_text(json.dumps(summary, indent=2))
print('TOTAL', summary['totals'], 'successful=', summary['successful'], flush=True)
sys.exit(0 if summary['successful'] else 1)
