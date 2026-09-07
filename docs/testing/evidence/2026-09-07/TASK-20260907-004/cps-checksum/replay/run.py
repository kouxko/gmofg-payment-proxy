#!/usr/bin/env python3
from pathlib import Path
import json,subprocess,sys,os,tempfile,hashlib
case=Path(__file__).resolve().parent.parent
root=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path.cwd()
state=json.loads((case/'inputs/tested-source-sha256.json').read_text())
for name,expected in state.items():
 p=root/name
 assert p.is_file() and hashlib.sha256(p.read_bytes()).hexdigest()==expected, f'Restore archived resources/source/{name} into a disposable checkout first'
out=Path(tempfile.mkdtemp(prefix='cps-checksum-replay-'))
env=os.environ.copy();env['CPS_REPO']=str(case/'resources/cps-source');env['GSON_JAR']=str(case/'resources/gson-2.9.0.jar');env['CPS_WIRE_EVIDENCE_DIR']=str(out/'wire')
for i,cmd in enumerate(json.loads((case/'replay/commands.json').read_text())):
 print('RUN',cmd,flush=True)
 with (out/f'{i:02}.log').open('w') as log:r=subprocess.run(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
 print('exit',r.returncode,'log',out/f'{i:02}.log',flush=True)
 if r.returncode:sys.exit(r.returncode)
subprocess.run([sys.executable,str(case/'replay/compare-wire.py'),str(out/'wire'),str(case/'resources/source/examples/protocol-packages/gmofg_cps/tests/fixtures/java-signatures.json')],check=True)
print('PASS',out)
