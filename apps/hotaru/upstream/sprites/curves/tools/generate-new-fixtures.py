#!/usr/bin/env python3
"""Generate only new records from actual pinned PNGs/reference, never Rust."""
import ctypes,hashlib,itertools,json,pathlib,struct
root=pathlib.Path(__file__).resolve().parent.parent
ref=pathlib.Path('/home/callen/work/ghostty-reference-20261028')
report=json.loads((root/'artifacts/final-expanded.json').read_text())
lib=root/'oracle-expanded/libsprites.so'
assert hashlib.sha256(lib.read_bytes()).hexdigest()==report['oracle_sha256']
fn=ctypes.CDLL(str(lib)).sprite_raster;fn.argtypes=[ctypes.c_uint32]*4+[ctypes.POINTER(ctypes.c_ubyte),ctypes.c_size_t];fn.restype=ctypes.c_int
from PIL import Image
data=bytearray(b'HSP1');records=[]
def add(cp,w,h,t,raw,source):
 encoded=b''.join(struct.pack('<HB',len(list(run)),value) for value,run in itertools.groupby(raw))
 data.extend(struct.pack('<IHHHI',cp,w,h,t,len(encoded)));data.extend(encoded)
 records.append({'cp':f'U+{cp:04X}','metrics':[w,h,t],'source':source,'raw_sha256':hashlib.sha256(raw).hexdigest()})
for block,cps in [(0x2500,range(0x256d,0x2574)),(0xe000,[*range(0xe0b0,0xe0c0),0xe0d2,0xe0d4])]:
 for w,h,t in [(9,17,1),(11,21,2),(12,24,3),(18,36,4)]:
  path=ref/f'src/font/sprite/testdata/U+{block:04X}...U+{block+255:04X}-{w}x{h}+{t}.png'
  assert hashlib.sha256(path.read_bytes()).hexdigest()==report['png_sha256'][str(path)]
  image=Image.open(path);sx=w+2*(w//4);sy=h+2*(h//4)
  for cp in cps:
   x=(cp-block)%16*sx;y=(cp-block)//16*sy;raw=image.crop((x,y,x+sx,y+sy)).tobytes()
   out=(ctypes.c_ubyte*len(raw))();assert fn(cp,w,h,t,out,len(raw))==0 and bytes(out)==raw
   add(cp,w,h,t,raw,str(path.relative_to(ref)))
for f in json.loads((root/'artifacts/initial-curves.json').read_text())['failures']:
 cp=int(f['cp'],16);w,h,t=f['metrics'];length=(w+2*(w//4))*(h+2*(h//4));out=(ctypes.c_ubyte*length)()
 assert fn(cp,w,h,t,out,length)==0
 add(cp,w,h,t,bytes(out),'real reference translation-order regression')
output=root/'rust/tests/fixtures/curves-powerline.rle';output.write_bytes(data)
output.with_suffix('.json').write_text(json.dumps({'ghostty_pin':report['upstream_pin'],'oracle_sha256':report['oracle_sha256'],'format':'same HSP1 lossless RLE as ghostty.rle','records':records,'sha256':hashlib.sha256(data).hexdigest()},indent=2)+'\n')
print(json.dumps({'records':len(records),'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}))
