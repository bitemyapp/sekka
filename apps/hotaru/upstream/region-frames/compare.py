#!/usr/bin/env python3
"""Remote-only bounded software frame comparison; preserve all trials."""
import datetime, fcntl, importlib.util, json, os, sys
from pathlib import Path

root = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('matrix', root/'tools/frame-matrix.py')
matrix = importlib.util.module_from_spec(spec); spec.loader.exec_module(matrix)
hotaru = Path('/nix/store/dkqws66ci8la92b6k2zsvkjy20a1yr71-hotaru-0.1.0')
ghostty = Path('/nix/store/rvbnq9b2j16127m46g7ia1vv9zw5cy03-ghostty-1.4.1-dev+6ba144158577d033ccb6ed4b574e4013957c9161-nix')
apps = {
    'copy': ('hotaru', hotaru, '0', '1d4e649540eed06b5ec4b43e756e1d9b5b8bdbb9a715487903f3613963d0b262', 1349, 1077),
    'redraw': ('hotaru', hotaru, '1', '1d4e649540eed06b5ec4b43e756e1d9b5b8bdbb9a715487903f3613963d0b262', 1349, 1077),
    'ghostty': ('ghostty', ghostty, '0', 'ea356b4e5f33e0d0d55af99bb978afe419eed2a933edd31e457554db8970f61f', 1334, 964),
}
roles = {'app':'24-27','compositor':'28-29','observer':'30','controller':'31','fixture':'32'}
cpus = set.union(*(matrix.cpu_list(v) for v in roles.values()))
topology = {str(cpu):Path(f'/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list').read_text().strip() for cpu in cpus}
cpus.update(set.union(*(matrix.cpu_list(v) for v in topology.values())))
report = {'schema':1,'status':'running','recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
          'scope':'two balanced software private-Sway rounds; copy enabled/disabled on identical Hotaru binary plus unchanged Ghostty; no hardware/physical scanout claim',
          'roles':roles,'topology':topology,'sources':{p.name:matrix.sha(p) for p in (root/'tools').iterdir() if p.is_file()},
          'controller_sha256':matrix.sha(__file__),'calibration':{},'trials':[]}
def save():
    (root/'result.json').write_text(json.dumps(report,indent=2)+'\n')
def trial(label, directory, purpose):
    kind, package, scroll_disabled, expected, width, height = apps[label]
    binary = package/'bin'/kind
    executable = package/'bin'/('.'+kind+'-wrapped')
    assert matrix.sha(executable)==expected
    directory.mkdir()
    receipt = {'label':label,'purpose':purpose,'no_pixel_scroll':scroll_disabled,'preflight':matrix.preflight(cpus)}
    if not receipt['preflight']['passed']:
        receipt.update(accepted=False,rejection_reasons=receipt['preflight']['reasons'])
        return receipt, None
    command = [sys.executable,str(root/'tools/frame-benchmark.py'),kind,str(binary),
               '--purpose',purpose,'--output',str(directory/'run'),'--lock',str(root/'child.lock'),
               '--font-dir','/nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts',
               '--fallback-dir','/nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts',
               '--mesa','/nix/store/457mqa9dsvw9cnmi6pr06zjy1ib0iic4-mesa-26.2.5',
               '--window-width',str(width),'--window-height',str(height),'--cell-width','11','--cell-height','24',
               '--keys','8' if purpose=='qualification' else '64',
               '--frames','4' if purpose=='qualification' else '120',
               '--control-seconds','.1' if purpose=='qualification' else '1',
               '--no-pixel-scroll',scroll_disabled]
    if kind=='ghostty': command.append('--ghostty-cell-height=-1')
    for name,value in roles.items():command.extend(['--'+name+'-cpus',value])
    receipt['command']=command
    old=os.environ.get('AIZOME_NO_PIXEL_SCROLL')
    os.environ['AIZOME_NO_PIXEL_SCROLL']=scroll_disabled
    try:code,observations,errors=matrix.run_trial(command,directory,cpus)
    finally:
        if old is None:os.environ.pop('AIZOME_NO_PIXEL_SCROLL',None)
        else:os.environ['AIZOME_NO_PIXEL_SCROLL']=old
    result_path=directory/'run/result.json'
    result=json.loads(result_path.read_text()) if result_path.exists() else {}
    reasons=list(errors)
    if code or result.get('status')!='passed':reasons.append('harness failed')
    if result.get('running_executable_sha256')!=expected:reasons.append('runtime executable mismatch')
    if result.get('binary_sha256')!=matrix.sha(binary):reasons.append('wrapper mismatch')
    if result.get('environment',{}).get('AIZOME_NO_PIXEL_SCROLL')!=scroll_disabled:reasons.append('pixel-copy control not applied')
    startup=result.get('startup',{}); geometry=startup.get('geometry',{})
    if not startup.get('qualified') or not result.get('physical_geometry_qualified') or (
        geometry.get('cell_width'),geometry.get('cell_height'),geometry.get('rows'))!=(11,24,40):
        reasons.append('unqualified startup/final geometry')
    if purpose=='measurement':
        original=report['calibration'][label]['result']
        if result.get('fonts')!=original.get('fonts') or result.get('configuration')!=original.get('configuration'):
            reasons.append('font/configuration differs from calibration')
    start=startup.get('launch_ns',0)
    end=max([startup.get('matching_frame',{}).get('ready_ns',0)]+[s['last_capture_ns'] for s in result.get('streams',[])])
    try:overlap=matrix.compiler_overlap(observations,start,end)
    except ValueError as error:overlap=[];reasons.append(str(error))
    if overlap:reasons.append('observed compiler overlap')
    receipt.update(accepted=not reasons,rejection_reasons=reasons,returncode=code,
                   measured_start_ns=start,measured_end_ns=end,compiler_overlap_samples=len(overlap),
                   result_sha256=matrix.sha(result_path) if result_path.exists() else None,
                   activity_sha256=matrix.sha(directory/'activity.jsonl'))
    return receipt,result

matrix.enable_subreaper()
try:
    with Path('/home/callen/.cache/hotaru-workstation.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        for label in apps:
            receipt,result=trial(label,root/('calibration-'+label),'qualification')
            report['calibration'][label]={'receipt':receipt,'result':result};save()
            if not receipt['accepted']:raise RuntimeError('calibration rejected: '+label)
        assert report['calibration']['copy']['result']['configuration']==report['calibration']['redraw']['result']['configuration']
        assert report['calibration']['copy']['result']['fonts']==report['calibration']['ghostty']['result']['fonts']
        for round_id,order in enumerate([['copy','redraw','ghostty'],['ghostty','redraw','copy']]):
            for label in order:
                receipt,_=trial(label,root/f'{round_id:02}-{label}','measurement')
                receipt['round']=round_id;report['trials'].append(receipt);save()
                if not receipt['accepted']:raise RuntimeError('trial rejected: '+label)
        report['status']='passed'
except BaseException as error:
    report.update(status='failed',error=repr(error));raise
finally:
    save()
    print(json.dumps({'status':report['status'],'trials':len(report['trials'])}))
