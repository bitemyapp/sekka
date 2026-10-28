#!/usr/bin/env python3
"""Generate triangle PNG and causal numerical expectations from actual upstream."""
import ctypes,hashlib,itertools,json,pathlib,struct
from PIL import Image
r=pathlib.Path(__file__).resolve().parent.parent
ref=pathlib.Path('/home/callen/work/ghostty-reference-20261028')
report=json.loads((r/'artifacts/hypot-geometric-verified.json').read_text())
lib=r/'oracle-expanded/libsprites.so';assert hashlib.sha256(lib.read_bytes()).hexdigest()==report['oracle_sha256']
f=ctypes.CDLL(str(lib)).sprite_raster;f.argtypes=[ctypes.c_uint32]*4+[ctypes.POINTER(ctypes.c_ubyte),ctypes.c_size_t];f.restype=ctypes.c_int
blob=bytearray(b'HSP1');records=[]
def add(cp,w,h,t,raw,source):
    runs=[]
    for a,items in itertools.groupby(raw):
        count=len(list(items))
        while count:
            take=min(count,65535);runs.append(struct.pack('<HB',take,a));count-=take
    data=b''.join(runs);blob.extend(struct.pack('<IHHHI',cp,w,h,t,len(data)));blob.extend(data)
    records.append({'cp':f'U+{cp:04X}','metrics':[w,h,t],'source':source,'raw_sha256':hashlib.sha256(raw).hexdigest()})
cps=[*range(0x25e2,0x25e6),*range(0x25f8,0x25fb),0x25ff]
for w,h,t in [(9,17,1),(11,21,2),(12,24,3),(18,36,4)]:
    path=ref/f'src/font/sprite/testdata/U+2500...U+25FF-{w}x{h}+{t}.png';assert hashlib.sha256(path.read_bytes()).hexdigest()==report['png_sha256'][str(path)]
    image=Image.open(path);sx=w+2*(w//4);sy=h+2*(h//4);assert image.mode=='L' and image.size==(16*sx,16*sy)
    for cp in cps:
        x=(cp-0x2500)%16*sx;y=(cp-0x2500)//16*sy;raw=image.crop((x,y,x+sx,y+sy)).tobytes();out=(ctypes.c_ubyte*len(raw))();assert f(cp,w,h,t,out,len(raw))==0 and bytes(out)==raw
        add(cp,w,h,t,raw,str(path.relative_to(ref)))
for case in json.loads((r/'artifacts/initial-geometric.json').read_text())['failures']:
    cp=int(case['cp'],16);w,h,t=case['metrics'];length=(w+2*(w//4))*(h+2*(h//4));out=(ctypes.c_ubyte*length)();assert f(cp,w,h,t,out,length)==0
    add(cp,w,h,t,bytes(out),'actual-library hypot rounding regression')
for w,h,t in [(3,4,2),(6,8,4),(5,12,4),(8,15,6),(7,24,6),(9,12,6),(1,1,1),(1,4096,1),(4096,1,1)]:
    length=(w+2*(w//4))*(h+2*(h//4));out=(ctypes.c_ubyte*length)()
    for cp in [*range(0x25f8,0x25fb),0x25ff]:
        assert f(cp,w,h,t,out,length)==0
        add(cp,w,h,t,bytes(out),'actual-library collapsed/thin inset boundary')
p=r/'rust/tests/fixtures/geometric.rle';p.write_bytes(blob);p.with_suffix('.json').write_text(json.dumps({'ghostty_pin':report['upstream_pin'],'oracle_sha256':report['oracle_sha256'],'records':records,'sha256':hashlib.sha256(blob).hexdigest()},indent=2)+'\n')
print(json.dumps({'records':len(records),'bytes':len(blob),'sha256':hashlib.sha256(blob).hexdigest()}))
