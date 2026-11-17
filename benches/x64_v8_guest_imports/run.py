"""Use interleaved fresh V8 isolates for the exact guest module used by Rust."""
from pathlib import Path
import argparse,json,subprocess,statistics,random,hashlib,platform
p=argparse.ArgumentParser();p.add_argument('output',type=Path);p.add_argument('--rounds',type=int,default=9);a=p.parse_args()
s=Path(__file__).resolve().parent;rows=[]
cases=['guest_loop','sync_import','promising_loop','promising_plain_import','promising_marked_scalar','promising_marked_pending']
for i in range(a.rounds):
    order=cases[:];random.Random(7195+i).shuffle(order)
    for case in order:
        r=json.loads(subprocess.check_output(['node','--no-liftoff',s/'imports.js',case],text=True));r['block']=i+1;rows.append(r)
a.output.mkdir(parents=True,exist_ok=True)
(a.output/'rows.json').write_text(json.dumps(rows,indent=2)+'\n')
out={k:statistics.median(r['ns_per_step'] for r in rows if r['kind']==k) for k in cases}
meta={'ns_per_step':out,'hashes':{f:hashlib.sha256((s/f).read_bytes()).hexdigest() for f in ['loop.wasm','import.wasm','imports.js','run.py']},'platform':platform.machine(),'blocks':a.rounds}
(a.output/'summary.json').write_text(json.dumps(meta,indent=2)+'\n')
print(json.dumps(meta))
