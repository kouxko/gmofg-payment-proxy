from pathlib import Path
import subprocess,urllib.request,json,zipfile,shutil,datetime
ROOT=Path(__file__).resolve().parents[1]
raw=subprocess.run(['git','credential','fill'],input='protocol=http\nhost=172.16.3.60\n\n',text=True,capture_output=True,check=True)
cred=dict(l.split('=',1) for l in raw.stdout.splitlines() if '=' in l)
headers={'PRIVATE-TOKEN':cred['password']};base='http://172.16.3.60/api/v4/projects/1023'
def get(path,name):
 with urllib.request.urlopen(urllib.request.Request(base+path,headers=headers),timeout=30) as r:data=json.load(r)
 (ROOT/'outputs'/name).write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n');return data
project=get('','project.json');release=get('/releases/v1.0.0','release.json')
pipes=get('/pipelines?per_page=30','pipelines.json')
for p in pipes:
 if p['status']!='skipped':get('/pipelines/'+str(p['id'])+'/jobs?per_page=100',f"jobs-{p['id']}.json")
assets=[]
for link in release['assets']['links']:
 target=ROOT/'resources'/link['url'].rsplit('/',1)[-1]
 with urllib.request.urlopen(urllib.request.Request(link['url'],headers=headers),timeout=60) as r,target.open('wb') as f:shutil.copyfileobj(r,f)
 result={'name':link['name'],'url':link['url'],'path':str(target.relative_to(ROOT)),'bytes':target.stat().st_size}
 result['file']=subprocess.check_output(['file',str(target)],text=True).strip()
 if zipfile.is_zipfile(target):
  with zipfile.ZipFile(target) as z:
   bad=z.testzip();result['zip_crc_valid']=bad is None;result['entries']=z.namelist();assert bad is None
 assets.append(result);print(link['name'],target.stat().st_size,flush=True)
report={'checked_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'tag':release['tag_name'],'commit':release['commit']['id'],'assets':assets,'macos_release_asset_present':any('mac' in a['name'].lower() or a['path'].endswith('.dmg') for a in assets),'read_only':True}
(ROOT/'outputs'/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');print('Existing release assets inspected; no remote mutations',flush=True)
