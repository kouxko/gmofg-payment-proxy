"""Verify archived wire bytes and protocol-stage linkage without running the App."""
from pathlib import Path
import json
CASE = Path(__file__).resolve().parents[1]
OUT = CASE / 'outputs/wire'
IN = CASE / 'inputs/wire'
def read(path):
    return json.loads(path.read_text())
def body(path):
    return path.read_bytes().split(b'\r\n\r\n', 1)[1]
summary = read(OUT / 'summary.json')
assert summary['result'] == 'PASS'
count = 0
for folder in (OUT, IN):
    for binary in folder.rglob('*.bin'):
        assert binary.read_bytes() == bytes.fromhex(binary.with_suffix('.hex').read_text()), binary
        count += 1
checks = []
for protocol, names in [('http', ['method', 'header', 'path', 'body', 'all', 'miss']), ('socket', ['match', 'miss'])]:
    observations = read(OUT / 'mcp' / (protocol + '-observations.json'))['rows']
    for name in names:
        folder = OUT / protocol / name
        client = read(folder / 'client-result.json')
        row = next(row for row in observations if row['peer_address'].endswith(':' + str(client['local_port'])))
        assert not row['evidence_evicted']
        events = {(event.get('direction'), event['event']): event for event in row['events']}
        assert events[(None, 'closed')]['outcome'] == 'completed'
        if protocol == 'http':
            assert events[('upstream', 'received')]['context']['body'].encode() == body(IN / 'http' / name / 'client-request.bin')
            upstream = body(folder / 'server-request.bin')
            response = body(folder / 'server-response.bin')
            for stage in ('encoded', 'sent'):
                assert events[('upstream', stage)]['context']['body'].encode() == upstream
            for stage in ('received', 'encoded', 'sent'):
                assert events[('downstream', stage)]['context']['body'].encode() == response
            assert body(folder / 'client-response.bin') == response
            server = read(folder / 'server-record.json')
            method, target, _ = (IN / 'http' / name / 'client-request.bin').read_bytes().split(b'\r\n', 1)[0].decode().split(' ', 2)
            assert (server['method'], server['target']) == (method, target)
            assert server['headers']['host'] == ['127.0.0.1:28081']
        else:
            request = (IN / 'socket' / name / 'client-frame.bin').read_bytes()
            actual = (folder / 'server-frame.bin').read_bytes()
            expected = request[:2] + b'0100' + request[6:] if name == 'match' else request
            assert actual == expected == (folder / 'client-response.bin').read_bytes()
            assert bytes(events[('upstream', 'received')]['context']['bytes']) == request
            assert events[('upstream', 'received')]['document']['message_type'] == ('0200' if name == 'match' else '0400')
            for direction, stages in [('upstream', ('encoded', 'sent')), ('downstream', ('received', 'encoded', 'sent'))]:
                for stage in stages:
                    assert bytes(events[(direction, stage)]['context']['bytes']) == expected
        checks.append(protocol + ':' + name)
assert read(CASE / 'outputs/persistence/comparison.json')['result'] == 'PASS'
assert read(CASE / 'outputs/cleanup.json')['result'] == 'PASS'
print(json.dumps({'result':'PASS','binary_hex_pairs':count,'linked_successful_wire_chains':checks,'persistence':'PASS','cleanup':'PASS'},indent=2))
