from pathlib import Path
import fcntl,subprocess
root=Path(__file__).resolve().parent.parent
with (Path.home()/'.cache/hotaru-workstation.lock').open('a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 for command in [
  ['cargo','+1.95.0','fmt','--all'],
  ['cargo','+1.95.0','test','-p','hotaru-core','--test','utf8_ascii_runs'],
  ['cargo','+1.95.0','test','--workspace'],
  ['cargo','+1.95.0','clippy','--workspace','--all-targets','--','-D','warnings']]:
  print(command,flush=True);subprocess.run(command,cwd=root,check=True)
