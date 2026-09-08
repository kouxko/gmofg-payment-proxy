from pathlib import Path
import importlib.util
import json
import sys
import time
ROOT = Path(__file__).resolve().parents[7]
CASE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from e2e_macos_mounted_release import mcp_call
OLD = ROOT / 'docs/testing/evidence/2026-09-01/TASK-20260901-001/release-app-replay/replay'
def load(name):
    spec = importlib.util.spec_from_file_location(name, OLD / (name + '_matrix.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module
http, socket = load('http'), load('socket')
http.PROXY_PORT, http.UPSTREAM_PORT = 28080, 28081
socket.PROXY_PORT, socket.UPSTREAM_PORT = 28082, 28083
out = CASE / 'outputs/configuration'
out.mkdir(exist_ok=True)
inputs = CASE / 'inputs/configuration'
inputs.mkdir(parents=True, exist_ok=True)
def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
selected = next(w for w in mcp_call(17653, 1, 'workspace_list', {}) if w['selected'])
value = http.candidate(selected['id'], selected['revision'])
second = socket.candidate(selected['id'], selected['revision'])
value['workspace']['listeners'] += second['workspace']['listeners']
value['workspace']['rules'] += second['workspace']['rules']
settings = value['workspace']['listeners'][0]['data_plane']['settings']
settings['topology'] = {'mode': 'remote_server', 'settings': {'fixed_server': settings.pop('fixed_server')}}
save(inputs / 'candidate.json', value)
preview = mcp_call(17653, 2, 'environment_candidate_create', {'candidate': value})
save(out / 'preview.json', preview)
assert preview['status'] == 'preview_ready', preview
queued = mcp_call(17653, 3, 'environment_candidate_apply', {'candidate_id': preview['candidate_id'], 'confirmation_token': preview['confirmation_token']})
save(out / 'queued.json', queued)
for attempt in range(100):
    status = mcp_call(17653, 4, 'environment_candidate_status', {'candidate_id': preview['candidate_id']})
    if status['status'] == 'committed':
        break
    assert status['status'] in ('apply_queued', 'apply_in_progress'), status
    time.sleep(.1)
else:
    raise TimeoutError('Candidate commit timeout')
save(out / 'committed.json', status)
save(out / 'workspace-before.json', mcp_call(17653, 5, 'workspace_get', {'workspace_id': selected['id']}))
save(out / 'entries-before.json', mcp_call(17653, 6, 'entry_status_list', {}))
print(json.dumps(status, ensure_ascii=False))
