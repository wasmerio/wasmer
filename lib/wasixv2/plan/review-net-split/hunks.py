import sys,re,json
d=open(sys.argv[1]).read().split('\n')
files=[];cur=None
for line in d:
    if line.startswith('diff --git'):
        cur={'header':[line],'hunks':[]};files.append(cur)
    elif cur is not None and not cur['hunks'] and not line.startswith('@@'):
        cur['header'].append(line)
    elif line.startswith('@@'):
        cur['hunks'].append([line])
    elif cur is not None and cur['hunks']:
        cur['hunks'][-1].append(line)
out=[];i=0
for f in files:
    name=f['header'][0].split(' b/')[-1]
    for h in f['hunks']:
        while h and h[-1]=='' : h.pop()
        out.append({'id':i,'file':name,'header':f['header'],'hunk':h});i+=1
json.dump(out,open(sys.argv[2],'w'))
for o in out:
    ch=[l for l in o['hunk'][1:] if l[:1] in '+-' ]
    print(f"{o['id']:3} {o['file'].split('/wasixv2/')[-1]} | "+' ‖ '.join(l[:70] for l in ch[:3]))
