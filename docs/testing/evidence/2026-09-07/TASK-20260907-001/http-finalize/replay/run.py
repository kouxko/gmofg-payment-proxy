#!/usr/bin/env python3
import json, os, pathlib, subprocess, sys, tempfile
case=pathlib.Path(__file__).resolve().parent.parent
root=pathlib.Path(sys.argv[1]).resolve() if len(sys.argv)>1 else pathlib.Path.cwd()
assert (root/'src-tauri/Cargo.toml').is_file(), 'Pass the repository root as the first argument'
out=pathlib.Path(tempfile.mkdtemp(prefix='http-finalize-replay-'))
env=os.environ.copy(); env['HTTP_FINALIZE_WIRE_EVIDENCE_DIR']=str(out/'wire')
for i, command in enumerate(json.loads((case/'replay/commands.json').read_text())):
    print('RUN', command, flush=True)
    result=subprocess.run(command,cwd=root,env=env,capture_output=True,text=True)
    (out/f'{i:02d}.log').write_text(result.stdout+result.stderr)
    print('exit',result.returncode,'log',out/f'{i:02d}.log',flush=True)
    if result.returncode: sys.exit(result.returncode)
config=out/'tsconfig.json'
config.write_text(json.dumps({'extends':str(root/'tsconfig.json'),'compilerOptions':{'incremental':False},'exclude':[str(root/'node_modules'),str(root/'docs/testing/evidence')]}))
result=subprocess.run(['deno','task','typecheck','--project',str(config)],cwd=root,capture_output=True,text=True)
(out/'source-typecheck.log').write_text(result.stdout+result.stderr)
print('Source typecheck exit',result.returncode,'outputs',out)
sys.exit(result.returncode)
