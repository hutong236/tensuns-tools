#!/usr/bin/env python3
import json,os,pathlib,subprocess,tempfile,threading
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from urllib.parse import urlparse,parse_qs
import sys
SCRIPT=pathlib.Path(sys.argv[1] if len(sys.argv)>1 else pathlib.Path(__file__).resolve().parents[1]/'target'/'debug'/('tensuns.exe' if os.name=='nt' else 'tensuns')).resolve()
def row(i,g='bulk',tags=None,meta=None):return dict(ID=i,name=g,address='192.0.2.1',port=9100,tags=tags or [],meta=meta or {})
state={f'old-{i}':row(f'old-{i}',meta={'text':'x'*80}) for i in range(6000)}
state['special']=row('special','special',['a,b','a|b'],{'desc':'a,b|c\n中文'})
writes=[];fail_id='';fail_get=False
class H(BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def send(self,d,s=200):
  b=json.dumps(d,ensure_ascii=False).encode();self.send_response(s);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b)
 def do_GET(self):
  if self.headers.get('Content-Type'):return self.send({'message':'unexpected JSON header'},400)
  if self.headers.get('Authorization')!='Bearer mock':return self.send({'code':50000,'data':'auth'})
  u=urlparse(self.path)
  if u.path=='/api/consul/services_name':return self.send({'code':20000,'services_name':sorted({x['name'] for x in state.values()})})
  if u.path=='/api/consul/instances':
   if fail_get:return self.send({'message':'mock HTTP 400'},400)
   g=parse_qs(u.query)['service_name'][0];a=[]
   for x in state.values():
    if x['name']==g:a.append(dict(x,tags=x['tags'] or '无',meta=[x['meta']] if x['meta'] else '无'))
   return self.send({'code':20000,'instances':a})
  return self.send({},404)
 def do_POST(self):
  p=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  if self.path=='/api/user/login':
   assert p=={'username':'admin','password':'mock password'};return self.send({'code':20000,'data':{'token':'Bearer mock'}})
  assert self.path=='/api/consul/sid' and self.headers.get('Authorization')=='Bearer mock' and self.headers.get('Content-Type')=='application/json'
  p=p['instance_dict'];writes.append(p['ID'])
  if p['ID']==fail_id:return self.send({'code':50000,'data':'mock failure'})
  assert p['checkInfo']['isCheck'] is False
  state[p['ID']]=dict(ID=p['ID'],name=p['name'],address=p['address'],port=p['port'],tags=p['tags'],meta=json.loads(p['metaInfo']['metaJson']) if p['metaInfo']['isMeta'] else {})
  return self.send({'code':20000})
s=ThreadingHTTPServer(('127.0.0.1',0),H);threading.Thread(target=s.serve_forever,daemon=True).start()
original_dir=os.getcwd()
with tempfile.TemporaryDirectory() as d:
 os.chdir(d);env=dict(os.environ,TENSUNS_URL=f'http://127.0.0.1:{s.server_port}',TENSUNS_USER='admin',NO_PROXY='127.0.0.1,localhost');env.pop('TENSUNS_TOKEN',None)
 def run(*args,rc=0):
  p=subprocess.run([str(SCRIPT),*args],input='mock password\n',text=True,capture_output=True,env=env,timeout=30,encoding='utf-8');assert p.returncode==rc,(args,p.returncode,p.stdout,p.stderr);return p
 def file(n,t):pathlib.Path(n).write_text(t,encoding='utf-8')
 def statuses(n):return [json.loads(x)['status'] for x in pathlib.Path(n).read_text(encoding='utf-8').splitlines()]
 file('new.txt','new|new-1|192.0.2.1|9100||\nnew|new-2|192.0.2.1|9100|a,b|env=prod\n')
 run('validate','new.txt');print('PASS local validation',flush=True)
 run('import','new.txt','--report','plan.jsonl');assert not writes;print('PASS 6,001 existing instances / large JSON dry run',flush=True)
 run('import','new.txt','--apply','--report','apply.jsonl');assert writes==['new-1','new-2'] and statuses('apply.jsonl').count('verified')==2;print('PASS creation / empty tags / readback',flush=True)
 run('new.txt','--apply','--report','again.jsonl');assert len(writes)==2;print('PASS legacy syntax / idempotence',flush=True)
 file('conflict.txt','new|old-0|192.0.2.1|9100||\nnew|new-3|192.0.2.1|9100||\n');run('import','conflict.txt','--report','conflict-plan.jsonl',rc=2);run('import','conflict.txt','--apply','--report','conflict.jsonl',rc=1);assert len(writes)==2
 run('import','conflict.txt','--apply','--skip-conflicts','--report','skip.jsonl');assert writes[-1]=='new-3';print('PASS conflict stop / skip',flush=True)
 for ext in ['json','jsonl','csv','xlsx']:
  run('export',f'out.{ext}','--service','special','--service','new');run('validate',f'out.{ext}');run('import',f'out.{ext}','--report',f'round-{ext}.jsonl');assert set(statuses(f'round-{ext}.jsonl'))=={'unchanged'};print('PASS roundtrip '+ext,flush=True)
 run('export','out.txt','--service','new');run('validate','out.txt');run('export','bad.txt','--service','special',rc=1);assert not pathlib.Path('bad.txt').exists();print('PASS TXT and refusal of lossy export',flush=True)
 run('export','out.txt','--service','new',rc=1);run('export','out.txt','--service','new','--force');print('PASS no-clobber / force',flush=True)
 assert json.loads(run('diff','new.txt','--changes-only').stdout)['counts']['unchanged']==2;print('PASS CLI diff',flush=True)
 run('export','all.json');assert len(json.loads(pathlib.Path('all.json').read_text(encoding='utf-8')))==len(state);assert 'bulk\t6000' in run('list','--service','bulk').stdout;print('PASS full export / count',flush=True)
 file('dup.txt','x|dupe|192.0.2.1|9100||\ny|dupe|192.0.2.1|9100||\n');run('validate','dup.txt',rc=1);print('PASS duplicate input',flush=True)
 file('fail.txt','x|first|192.0.2.1|9100||\nx|fail|192.0.2.1|9100||\nx|last|192.0.2.1|9100||\n');fail_id='fail';run('import','fail.txt','--apply','--report','fail.jsonl',rc=1);assert 'last' not in writes and {'failed_or_uncertain','verified','not_attempted'}<=set(statuses('fail.jsonl'));print('PASS business failure stops / records',flush=True)
 env['TENSUNS_TOKEN']='mock';assert 'bulk\t6000' in run('list','--service','bulk').stdout;env.pop('TENSUNS_TOKEN');print('PASS token authentication',flush=True)
 fail_get=True;p=run('import','new.txt','--report','http.jsonl',rc=1);assert 'mock HTTP 400' in p.stderr;print('PASS HTTP 400 diagnostics',flush=True)
 os.chdir(original_dir)
s.shutdown()
