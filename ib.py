import json
d=json.load(open('crates/verdigris-protocol/data/26.3/blocks.json'))
names={b['name']:b for b in d['blocks']}
for n in ['minecraft:tall_grass','minecraft:large_fern','minecraft:sunflower','minecraft:lilac','minecraft:rose_bush','minecraft:peony','minecraft:oak_door','minecraft:white_bed']:
    b=names.get(n)
    if not b:
        print(n,"MISSING"); continue
    props={p['name']:p['values'] for p in b['properties']}
    print(n,'id=',b['id'],props)

net=json.load(open('crates/verdigris-protocol/data/26.3/network.json'))
for tr in net['tags']:
    if tr['registry']=='minecraft:block':
        for t in tr['tags']:
            if t['name'] in ('minecraft:doors','minecraft:beds','minecraft:replaceable'):
                print(t['name'],'count=',len(t['ids']))
