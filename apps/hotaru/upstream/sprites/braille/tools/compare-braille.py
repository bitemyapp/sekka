#!/usr/bin/env python3
"""Exact Alpha8 checks against unchanged Ghostty drawing/Canvas/z2d code."""
import argparse,ctypes,datetime,fcntl,hashlib,json,math,os,pathlib,random,struct,subprocess
from PIL import Image

def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def supported(cp): return 0x2800 <= cp <= 0x28ff

def main():
 p=argparse.ArgumentParser();p.add_argument('--root',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);p.add_argument('--oracle',type=pathlib.Path);p.add_argument('--oracle-build-mode',default='ReleaseSafe');a=p.parse_args()
 root=a.root;lib=a.oracle or root/'oracle-expanded/libsprites.so';candidate=root/'rust/target/release/hotaru-sprite-prototype'
 upstream=pathlib.Path('/home/callen/work/ghostty-reference-20261028')
 oracle=ctypes.CDLL(str(lib));fn=oracle.sprite_raster;fn.argtypes=[ctypes.c_uint32]*4+[ctypes.POINTER(ctypes.c_ubyte),ctypes.c_size_t];fn.restype=ctypes.c_int
 cps=list(range(0x2800,0x2900))
 canonical=[(9,17,1),(11,21,2),(12,24,3),(18,36,4)]
 scales=[1,1.25,1.5,1.75,2,2.25,2.5,3]
 scaled=[(math.ceil(11*s),math.ceil(24*s),math.ceil(s)) for s in scales]
 metrics=set(canonical+scaled)
 for w in range(1,33):
  for h in range(1,65):metrics.add((w,h,1))
 rng=random.Random(0x280028ff)
 for _ in range(256):
  w=rng.randrange(1,129);h=rng.randrange(1,193);metrics.add((w,h,rng.randrange(1,min(w,h)+1)))
 for w,h in [(4096,1),(1,4096),(1365,1365),(4096,455),(455,4096),(255,257),(256,256),(257,255)]:
  for t in sorted({1,min(w,h),max(1,min(w,h)//2)}):metrics.add((w,h,t))
 report={'schema':1,'recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'padded Alpha8 raster only; no application, metrics derivation, atlas upload or presentation claim','upstream_pin':'6ba144158577d033ccb6ed4b574e4013957c9161','glyphs':len(cps),'metrics':len(metrics),'large_metrics_scope':'above256x256: blank, eight basis dots and full pattern; all256 elsewhere','fractional_scale_cases':[{'scale':s,'physical_metrics':m} for s,m in zip(scales,scaled)],'candidate_sha256':sha(candidate),'oracle_sha256':sha(lib),'oracle_build_mode':a.oracle_build_mode,'failures':[],'matches':0,'compared_bytes':0,'png_matches':0,'metadata_matches':0,'png_sha256':{},'load_average':os.getloadavg()}
 with pathlib.Path('/home/callen/.cache/hotaru-workstation.lock').open('a') as lock:
  fcntl.flock(lock,fcntl.LOCK_EX)
  for w,h,t in sorted(metrics):
   (root/"artifacts/current-metric.json").write_text(json.dumps({"metrics":[w,h,t],"completed_matches":report["matches"]})+"\n")
   length=(w+2*(w//4))*(h+2*(h//4));out=(ctypes.c_ubyte*length)()
   codes=cps if w*h<=256*256 else [0x2800,*[0x2800|(1<<b) for b in range(8)],0x28ff]
   payload=''.join(f'{cp} {w} {h} {t}\n' for cp in codes).encode()
   response=subprocess.run([str(candidate)],input=payload,capture_output=True,check=True).stdout
   at=0
   for cp in codes:
    size,rw,rh,rx,ry,ix,iy,iw,ih=struct.unpack_from('<IIIiiIIII',response,at);at+=36;actual=response[at:at+size];at+=size
    code=fn(cp,w,h,t,out,length)
    if code: raise RuntimeError(('oracle rejected',cp,w,h,t,code))
    expected=bytes(out)
    bbox=Image.frombytes('L',(w+2*(w//4),h+2*(h//4)),expected).getbbox()
    ink=(bbox[0],bbox[1],bbox[2]-bbox[0],bbox[3]-bbox[1]) if bbox else (0xffffffff,0,0,0)
    metadata=(rw,rh,rx,ry,ix,iy,iw,ih)
    expected_metadata=(w+2*(w//4),h+2*(h//4),-(w//4),-(h//4),*ink)
    if metadata!=expected_metadata:report['failures'].append({'kind':'metadata','cp':hex(cp),'metrics':[w,h,t],'actual':metadata,'expected':expected_metadata})
    else:report['metadata_matches']+=1
    if size!=length or actual!=expected:
     difference=next((i for i,(x,y) in enumerate(zip(actual,expected)) if x!=y),None)
     report['failures'].append({'cp':hex(cp),'metrics':[w,h,t],'first_byte':difference,'actual':actual[difference] if difference is not None else None,'expected':expected[difference] if difference is not None else None})
    else:report['matches']+=1
    report['compared_bytes']+=length
   assert at==len(response)
  for block,codes in [(0x2800,cps)]:
   for w,h,t in canonical:
    png=upstream/f'src/font/sprite/testdata/U+{block:04X}...U+{block+255:04X}-{w}x{h}+{t}.png'
    report['png_sha256'][str(png)]=sha(png);image=Image.open(png)
    assert image.mode=='L';sx=w+2*(w//4);sy=h+2*(h//4);assert image.size==(16*sx,16*sy)
    out=(ctypes.c_ubyte*(sx*sy))()
    for cp in codes:
     index=cp-block;x=index%16*sx;y=index//16*sy
     expected=image.crop((x,y,x+sx,y+sy)).tobytes()
     assert fn(cp,w,h,t,out,len(out))==0
     if bytes(out)!=expected:report['failures'].append({'png':str(png),'cp':hex(cp),'kind':'oracle disagrees with upstream committed raster'})
     else:report['png_matches']+=1
 report['sources']={str(p.relative_to(root)):sha(p) for tree in ('rust/src','rust/tests','oracle-expanded/src') for p in (root/tree).rglob('*') if p.is_file()}
 report['sources']['oracle-expanded/oracle.zig']=sha(root/'oracle-expanded/oracle.zig')
 report['status']='passed' if not report['failures'] else 'failed'
 a.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k] for k in ('status','matches','metrics','compared_bytes','png_matches','metadata_matches','failures')}))
 raise SystemExit(report['status']!='passed')
if __name__=='__main__':main()
