from pathlib import Path
import json, random, statistics, subprocess
root = Path(__file__).resolve().parent
modes = {'boundary': ['--no-turbo-inline-js-wasm-calls'], 'default-wasm': ['--turbo-inline-js-wasm-calls'], 'body-inlining': ['--turbo-inline-js-wasm-calls','--turboshaft-wasm-in-js-inlining']}
rows = []
for trial in range(7):
    cases = [(k,s) for k in modes for s in ('js','direct','reentry')]
    random.Random(7195 + trial).shuffle(cases)
    for mode, case in cases:
        output = subprocess.check_output(['node','--no-maglev','--allow-natives-syntax',*modes[mode],'boundary.js',case],cwd=root)
        value = json.loads(output)
        value.update(trial=trial,mode=mode)
        rows.append(value)
(root/'results.json').write_text(json.dumps(rows,indent=2))
for c in ('js', 'direct','reentry'):
    for m in modes:
        vals = [r['nsPerCall'] for r in rows if (r['kind'],r['mode']) == (c,m)]
        print(c, m, 'median/min/max',*(round(v,3) for v in (statistics.median(vals),min(vals),max(vals))))
