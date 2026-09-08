from pathlib import Path
import datetime
import json
import sys
ROOT = Path(__file__).resolve().parents[7]
CASE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from e2e_macos_mounted_release import mcp_call
out = CASE / 'outputs/persistence'
out.mkdir(exist_ok=True)
phase = sys.argv[1]
assert phase in ('before', 'after')
workspaces = mcp_call(17653, 1, 'workspace_list', {})
selected = next(w for w in workspaces if w['selected'])
workspace = mcp_call(17653, 2, 'workspace_get', {'workspace_id': selected['id']})
value = {'at': datetime.datetime.now().astimezone().isoformat(), 'workspaces': workspaces, 'workspace': workspace, 'entries': mcp_call(17653, 3, 'entry_status_list', {}), 'catalog': mcp_call(17653, 4, 'protocol_package_catalog', {})}
(out / (phase + '.json')).write_text(json.dumps(value, ensure_ascii=False, indent=2)+'\n')
if phase == 'after':
    before = json.loads((out / 'before.json').read_text())
    assert workspace == before['workspace'], 'Persisted workspace changed across restart'
    assert workspaces == before['workspaces'], 'Workspace selection/list changed across restart'
    assert len(workspace['listeners']) == 2 and len(workspace['rule_definitions']) == 5
    (out / 'comparison.json').write_text(json.dumps({'result':'PASS','workspace_byte_structure_equal':True,'workspace_list_selection_equal':True,'listeners':2,'rules':5,'runtime_entries_recorded_separately':True},indent=2)+'\n')
print(phase, 'snapshot saved', selected['id'])
