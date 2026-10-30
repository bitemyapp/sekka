#!/usr/bin/env python3
"""Check the isolated f64 translation against the exact pinned Zig std function."""
import ctypes,fcntl,hashlib,json,math,pathlib,random,struct,subprocess
root=pathlib.Path(__file__).resolve().parent.parent
bits=lambda x:struct.unpack('<Q',struct.pack('<d',x))[0]
value=lambda x:struct.unpack('<d',struct.pack('<Q',x))[0]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
cases=[(float(x),float(y)) for x in range(1,257) for y in range(1,257)]
rng=random.Random(0x25e225ff)
for _ in range(65536):
    cases.append((math.ldexp(rng.uniform(-2,2),rng.randrange(-55,16)),math.ldexp(rng.uniform(-2,2),rng.randrange(-55,16))))
edge=[0.,-0.,value(1),value(0x10000000000000),1e-200,1e-155,1e154,1e200,value(0x7fefffffffffffff),math.inf,-math.inf,math.nan]
cases.extend((x,y) for x in edge for y in edge)
with pathlib.Path('/home/callen/.cache/hotaru-workstation.lock').open('a') as lock:
    fcntl.flock(lock,fcntl.LOCK_EX)
    library=root/'artifacts/libhypot.so';binary=root/'rust/target/release/examples/hypot'
    f=ctypes.CDLL(str(library)).reference_hypot;f.argtypes=[ctypes.c_double,ctypes.c_double];f.restype=ctypes.c_double
    data=''.join(f'{bits(x):016x} {bits(y):016x}\n' for x,y in cases).encode()
    lines=subprocess.run([str(binary)],input=data,capture_output=True,check=True).stdout.splitlines()
    assert len(lines)==len(cases)
    mismatch=[];libm_difference=[]
    for (x,y),line in zip(cases,lines):
        ours,libm=[int(v,16) for v in line.split()];expected=f(x,y)
        equal=lambda b: math.isnan(expected) and math.isnan(value(b)) or bits(expected)==b
        record={'x_bits':f'{bits(x):016x}','y_bits':f'{bits(y):016x}','reference_bits':f'{bits(expected):016x}','rust_bits':f'{ours:016x}','libm_bits':f'{libm:016x}'}
        if not equal(ours):mismatch.append(record)
        if not equal(libm):libm_difference.append(record)
    report={'status':'passed' if not mismatch else 'failed','cases':len(cases),'comparison':'exactbits exceptNaN class','mismatches':mismatch,'platform_libm_differences':len(libm_difference),'first_libm_differences':libm_difference[:30],'library_sha256':sha(library),'candidate_sha256':sha(binary),'translation_sha256':sha(root/'rust/src/math.rs'),'adapter_sha256':sha(root/'tools/hypot.zig')}
    (root/'artifacts/hypot-numerical.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ['status','cases','platform_libm_differences','mismatches']}))
    raise SystemExit(bool(mismatch))
