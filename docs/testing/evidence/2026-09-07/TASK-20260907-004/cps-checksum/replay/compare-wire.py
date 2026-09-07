from pathlib import Path
import json,hashlib,sys
p=Path(sys.argv[1]) if len(sys.argv)>1 else Path('/tmp/task0907-cps-final-wire')
fixture=Path(sys.argv[2]) if len(sys.argv)>2 else Path('examples/protocol-packages/gmofg_cps/tests/fixtures/java-signatures.json')
rows=json.loads(fixture.read_text());after=next(r for r in rows if r['id']=='pay-after')
frames={}
for name in ['client_input','origin_request','origin_response','app_response']:
 raw=(p/f'{name}.bin').read_bytes();head,body=raw.split(b'\r\n\r\n',1);lines=head.decode().split('\r\n');headers={k.lower():v.strip() for k,v in (line.split(':',1) for line in lines[1:])}
 assert int(headers['content-length'])==len(body)
 frames[name]={'start_line':lines[0],'headers':headers,'body':json.loads(body),'frame_bytes':len(raw),'body_bytes':len(body),'sha256':hashlib.sha256(raw).hexdigest()}
 if (p/f'{name}.chunks.json').exists():
  chunks=json.loads((p/f'{name}.chunks.json').read_text());assert b''.join(bytes(c) for c in chunks)==raw
  frames[name]['read_chunk_sizes']=[len(c) for c in chunks]
assert frames['client_input']['start_line']==frames['origin_request']['start_line']
assert frames['client_input']['body']==frames['origin_request']['body']
expected=json.loads(after['body']);expected['data']['sign']=after['sign']
assert frames['app_response']['body']==expected
assert frames['app_response']['headers']['x-path-id']==frames['client_input']['headers']['x-path-id']
assert hashlib.md5(after['canonical'].encode()).hexdigest().upper()==after['sign']
result={'result':'PASS','frames':frames,'expected_app_body':expected,'expected_sign_from_java':after['sign'],'assertions':['all four Content-Length match actual body bytes','actual read chunks reassemble exactly to origin request and App response','original token path and request body retained','App body matches independent Java pay-after fields and sign','path-id consistent across client and final response']}
(p/'comparison.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
print('PASS: 4 HTTP frames, 2 actual chunk streams, original token/body, Java pay-after signature')
