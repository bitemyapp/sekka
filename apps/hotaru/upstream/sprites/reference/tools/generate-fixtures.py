#!/usr/bin/env python3
"""Extract lossless test RLE directly from pinned Ghostty's committed PNGs."""
import argparse, hashlib, itertools, json, pathlib, struct
from PIL import Image
p=argparse.ArgumentParser();p.add_argument('--upstream',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);a=p.parse_args()
expected_hashes={pathlib.Path(p).name:sha for p,sha in json.loads((pathlib.Path(__file__).resolve().parent.parent/'evidence/final.json').read_text())['png_sha256'].items()}
data=bytearray(b'HSP1');sources={};count=0
for w,h,t in [(9,17,1),(11,21,2),(12,24,3),(18,36,4)]:
 path=a.upstream/f'src/font/sprite/testdata/U+2500...U+25FF-{w}x{h}+{t}.png'
 sources[str(path)]=hashlib.sha256(path.read_bytes()).hexdigest();assert sources[str(path)]==expected_hashes[path.name], 'PNG differs from audited pin'
 image=Image.open(path);assert image.mode=='L'
 sx=w+2*(w//4);sy=h+2*(h//4);assert image.size==(16*sx,16*sy)
 for cp in range(0x2500,0x25a0):
  if 0x256d<=cp<=0x2573:continue
  index=cp-0x2500;x=index%16*sx;y=index//16*sy
  raw=image.crop((x,y,x+sx,y+sy)).tobytes()
  encoded=b''.join(struct.pack('<HB',len(list(run)),value) for value,run in itertools.groupby(raw))
  data.extend(struct.pack('<IHHHI',cp,w,h,t,len(encoded)));data.extend(encoded);count+=1
assert count==612
a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_bytes(data)
a.output.with_suffix('.json').write_text(json.dumps({'format':'HSP1 then cp:u32,width:u16,height:u16,thickness:u16,encoded_len:u32 then runs(length:u16,alpha:u8);alllittleendian','source':'committed Ghostty PNG crops, not Rust output','ghostty_pin':'6ba144158577d033ccb6ed4b574e4013957c9161','records':count,'sha256':hashlib.sha256(data).hexdigest(),'png_sources':sources},indent=2)+'\n')
