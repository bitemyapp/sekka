#!/usr/bin/env python3
"""Extract lossless Braille expectations from verified actual upstream PNGs."""
import ctypes,hashlib,itertools,json,pathlib,struct
from PIL import Image
root=pathlib.Path(__file__).resolve().parent.parent
ref=pathlib.Path('/home/callen/work/ghostty-reference-20261028')
report=json.loads((root/'artifacts/releasefast-braille.json').read_text())
lib=root/'oracle-expanded/libsprites-fast.so'
assert hashlib.sha256(lib.read_bytes()).hexdigest()==report['oracle_sha256']
fn=ctypes.CDLL(str(lib)).sprite_raster;fn.argtypes=[ctypes.c_uint32]*4+[ctypes.POINTER(ctypes.c_ubyte),ctypes.c_size_t];fn.restype=ctypes.c_int
blob=bytearray(b'HSP1');records=[]
for w,h,t in [(9,17,1),(11,21,2),(12,24,3),(18,36,4)]:
 path=ref/f'src/font/sprite/testdata/U+2800...U+28FF-{w}x{h}+{t}.png'
 assert hashlib.sha256(path.read_bytes()).hexdigest()==report['png_sha256'][str(path)]
 image=Image.open(path);sx=w+2*(w//4);sy=h+2*(h//4)
 assert image.mode=='L' and image.size==(16*sx,16*sy)
 for cp in range(0x2800,0x2900):
  x=(cp-0x2800)%16*sx;y=(cp-0x2800)//16*sy
  raw=image.crop((x,y,x+sx,y+sy)).tobytes()
  out=(ctypes.c_ubyte*len(raw))();assert fn(cp,w,h,t,out,len(out))==0 and bytes(out)==raw
  encoded=b''.join(struct.pack('<HB',len(list(run)),value) for value,run in itertools.groupby(raw))
  blob.extend(struct.pack('<IHHHI',cp,w,h,t,len(encoded)));blob.extend(encoded)
  records.append({'cp':f'U+{cp:04X}','metrics':[w,h,t],'png':str(path.relative_to(ref)),'raw_sha256':hashlib.sha256(raw).hexdigest()})
output=root/'rust/tests/fixtures/braille.rle';output.write_bytes(blob)
output.with_suffix('.json').write_text(json.dumps({'ghostty_pin':report['upstream_pin'],'oracle_build_mode':report['oracle_build_mode'],'oracle_sha256':report['oracle_sha256'],'format':'HSP1 lossless RLE; actual PNG expectations, never candidate-derived','sha256':hashlib.sha256(blob).hexdigest(),'records':records},indent=2)+'\n')
print(json.dumps({'records':len(records),'bytes':len(blob),'sha256':hashlib.sha256(blob).hexdigest()}))
