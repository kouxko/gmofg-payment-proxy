import datetime,json,os,platform,socket,subprocess,threading,time
from pathlib import Path
from zoneinfo import ZoneInfo
base=Path(__file__).resolve().parents[1]
def now():return datetime.datetime.now(ZoneInfo('Asia/Shanghai')).isoformat()
def run(cmd):
 r=subprocess.run(cmd,capture_output=True,text=True,timeout=20)
 return dict(command=cmd,exit_code=r.returncode,stdout=r.stdout,stderr=r.stderr,at=now())
meta=json.loads((base/'metadata.json').read_text())
assert meta['lifecycle']=='RUNNING' and meta['runner']['session_id']=='01a07fed-750e-7511-8509-fc0762cfbb31/root'
adb=['adb','-s','10.0.34.117:5555','shell']
state='date; content query --uri content://jp.gmofg.app.launcher.provider.settings/POSLINK_COMMUNICATION_TYPE; ss -lnt; cat /proc/net/tcp; cat /proc/net/tcp6; ip -4 rule show; ip -4 route show table all'
baseline=run(adb+[state]); baseline['local_environment']={'platform':platform.platform(),'python':platform.python_version(),'route':run(['route','-n','get','10.0.34.117']),'adb':run(['adb','version'])}
(base/'inputs/baseline.json').write_text(json.dumps(baseline,ensure_ascii=False,indent=2)+'\n')
(base/'inputs/probe.json').write_text(json.dumps({'path_id':'path-001','host':'10.0.34.117','port':7788,'timeout_seconds':8,'application_bytes_sent':0,'source_binding':'10.0.34.43','purpose':'TCP handshake only'},indent=2)+'\n')
caplog=(base/'outputs/tcpdump.stderr.txt').open('w')
capcmd=['tcpdump','-i','en0','-nn','-U','-w',str(base/'outputs/handshake.pcap'),'host','10.0.34.117','and','tcp','port','7788']
cap=subprocess.Popen(capcmd,stdout=caplog,stderr=caplog);time.sleep(.5)
result={'path_id':'path-001','started_at':now(),'application_bytes_sent':0,'capture_command':capcmd}
def observe():
 time.sleep(1)
 (base/'outputs/during.json').write_text(json.dumps(run(adb+['date; cat /proc/net/tcp; cat /proc/net/tcp6']),indent=2)+'\n')
thread=threading.Thread(target=observe);thread.start()
s=socket.socket();s.settimeout(8);s.bind(('10.0.34.43',0));result['local_endpoint']=s.getsockname();start=time.monotonic()
try:
 s.connect(('10.0.34.117',7788));result['tcp']='VERIFIED';result['connect_seconds']=time.monotonic()-start;time.sleep(2)
except OSError as e:result.update(tcp='FAILED',error_type=type(e).__name__,error=str(e),errno=e.errno,connect_seconds=time.monotonic()-start)
finally:
 s.close();result['socket_closed']=True;thread.join();
 if cap.poll() is None:cap.terminate()
 try:cap.wait(timeout=5)
 except subprocess.TimeoutExpired:cap.kill();cap.wait()
 caplog.close();result['capture_exit_code']=cap.returncode;result['completed_at']=now()
(base/'outputs/probe.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
(base/'outputs/after.json').write_text(json.dumps(run(adb+[state]),ensure_ascii=False,indent=2)+'\n')
print(json.dumps(result,ensure_ascii=False,indent=2))
print((base/'outputs/tcpdump.stderr.txt').read_text())
