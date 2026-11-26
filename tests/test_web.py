#!/usr/bin/env python3
"""Web integration test: real local binary, isolated mock upstream, no external services."""
import json, os, pathlib, socket, subprocess, sys, tempfile, threading, time, urllib.request, urllib.error
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

binary=pathlib.Path(sys.argv[1]).resolve()
def row(i,g='bulk'):return dict(ID=i,name=g,address='192.0.2.1',port=9100,tags=[],meta={})
state={f'old-{i}':row(f'old-{i}') for i in range(6001)}
writes=[]
target_state={'first':dict(row('first','other'),port=161),'extra':row('extra','new')}
target_writes=[]
fail_id=None
class Upstream(BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def send(self,d,s=200):
  b=json.dumps(d,ensure_ascii=False).encode();self.send_response(s);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b)
 def do_GET(self):
  if self.headers.get('Content-Type'):return self.send({'error':'unexpected Content-Type'},400)
  if self.headers.get('Authorization')!='Bearer mock':return self.send({'code':50000})
  u=urlparse(self.path);is_target=u.path.startswith('/target/');catalog=target_state if is_target else state;path=u.path.removeprefix('/target')
  if path=='/api/consul/services_name':return self.send({'code':20000,'services_name':sorted({r['name'] for r in catalog.values()})})
  if path=='/api/consul/instances':
   g=parse_qs(u.query)['service_name'][0]
   return self.send({'code':20000,'instances':[dict(r,tags=r['tags'] or '无',meta=[r['meta']] if r['meta'] else '无') for r in catalog.values() if r['name']==g]})
  return self.send({},404)
 def do_POST(self):
  data=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  is_target=self.path.startswith('/target/');path=self.path.removeprefix('/target');catalog=target_state if is_target else state;submitted=target_writes if is_target else writes
  if path=='/api/user/login':
   return self.send({'code':20000,'data':{'token':'Bearer mock'}} if data=={'username':'admin','password':'mock password'} else {'code':40000})
  assert path=='/api/consul/sid' and self.headers.get('Authorization')=='Bearer mock'
  d=data['instance_dict'];submitted.append(d['ID'])
  if d['ID']==fail_id:return self.send({'code':50000,'data':'mock failure'})
  assert d['checkInfo']['isCheck'] is False
  catalog[d['ID']]=dict(ID=d['ID'],name=d['name'],address=d['address'],port=d['port'],tags=d['tags'],meta=json.loads(d['metaInfo']['metaJson']) if d['metaInfo']['isMeta'] else {})
  return self.send({'code':20000})
up=ThreadingHTTPServer(('127.0.0.1',0),Upstream);threading.Thread(target=up.serve_forever,daemon=True).start()
with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
origin=f'http://127.0.0.1:{port}'
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
with tempfile.TemporaryDirectory() as d:
 env=dict(os.environ,NO_PROXY='127.0.0.1,localhost',TENSUNS_TOKEN='must-not-use-in-web')
 p=subprocess.Popen([str(binary),'--url',f'http://127.0.0.1:{up.server_port}','serve','--port',str(port),'--report-dir',d],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,env=env)
 token=''
 def req(path,body=None,expected=200,headers=None,auth=True):
  h=dict(headers or {})
  if body is not None:h.setdefault('Content-Type','application/json');h.setdefault('Origin',origin)
  if token and auth:h['X-TenSunS-Session']=token
  q=urllib.request.Request(origin+path,data=None if body is None else json.dumps(body).encode(),headers=h)
  try:
   with opener.open(q,timeout=30) as r:status=r.status;data=r.read();rh=r.headers
  except urllib.error.HTTPError as e:status=e.code;data=e.read();rh=e.headers
  assert status==expected,(path,body and body.get('action'),status,expected,data[:300])
  return data,rh
 def call(action,expected=200,**kw):
  return json.loads(req('/api/action',dict(action=action,**kw),expected)[0])
 try:
  for _ in range(100):
   try:req('/api/info');break
   except OSError:
    if p.poll() is not None:raise RuntimeError('Web server exited')
    time.sleep(.1)
  else:raise RuntimeError('Web server not ready')
  html,h=req('/');assert b'TenSunS Tools' in html and 'frame-ancestors' in h['Content-Security-Policy']
  assert b'fetch(' in req('/app.js')[0];req('/style.css')
  req('/api/info',expected=403,headers={'Host':'evil.example'})
  req('/api/action',{'action':'login','username':'admin','password':'mock password'},403,{'Origin':'https://evil.example'})
  call('groups',401)
  call('login',401,username='admin',password='wrong')
  token=call('login',username='admin',password='mock password')['session']
  req('/api/action',{'action':'groups'},401,auth=False)
  assert call('groups')['groups']==[{'name':'bulk','count':6001}]
  result=call('instances',service='bulk',query='old-1',page=1,page_size=20);assert len(result['rows'])==20 and result['total']>20
  print('PASS Web assets, Host/Origin, auth, large catalog, pagination',flush=True)
  data='new|first|192.0.2.1|9100||\nnew|second|192.0.2.1|9100|test|env=prod\n'
  plan=call('preview',format='txt',content=data);assert plan['counts']=={'planned':2} and not writes
  call('apply',400,ticket=plan['ticket'],confirm=False)
  result=call('apply',ticket=plan['ticket'],confirm=True);assert result['exit_code']==0 and result['counts']=={'verified':2} and writes==['first','second']
  call('apply',409,ticket=plan['ticket'],confirm=True)
  reports=call('reports')['reports'];assert len(reports)==1
  report=call('report',id=reports[0]['id']);assert 'verified' in report['content'];assert len(list(pathlib.Path(d).rglob('*.jsonl')))==1
  print('PASS preview, confirm, apply, verification, one-use ticket, report',flush=True)
  for fmt in ['txt','csv','json','jsonl','xlsx']:
   exported=call('export',format=fmt,services=['new']);assert exported['count']==2
   plan=call('preview',format=fmt,content=exported['content']);assert plan['counts']=={'unchanged':2}
  print('PASS five-format export/import roundtrip',flush=True)
  plan=call('preview',format='txt',content='new|third|192.0.2.1|9100||\n')
  state['third']=row('third','other');call('apply',409,ticket=plan['ticket'],confirm=True);assert 'third' not in writes
  plan=call('preview',format='txt',content='new|third|192.0.2.1|9100||\nnew|fourth|192.0.2.1|9100||\n');assert plan['counts']=={'conflict':1,'planned':1}
  call('apply',409,ticket=plan['ticket'],confirm=True);assert 'fourth' not in writes
  plan=call('preview',format='txt',content='new|third|192.0.2.1|9100||\nnew|fourth|192.0.2.1|9100||\n')
  assert call('apply',ticket=plan['ticket'],confirm=True,skip_conflicts=True)['exit_code']==0
  assert writes[-1]=='fourth'
  print('PASS stale plan and cross-service conflicts',flush=True)
  fail_id='fail'
  plan=call('preview',format='txt',content='new|fail|192.0.2.1|9100||\nnew|never|192.0.2.1|9100||\n')
  result=call('apply',ticket=plan['ticket'],confirm=True);assert result['exit_code']==1 and result['counts']=={'failed_or_uncertain':1,'not_attempted':1} and 'never' not in writes
  call('logout');call('groups',401);call('report',401,id=reports[0]['id']);token=call('login',username='admin',password='mock password')['session'];assert len(call('reports')['reports'])>=2;assert 'verified' in call('report',id=reports[0]['id'])['content'];assert call('template')['encoding']=='base64'
  print('PASS failed write stop, logout and persistent reports',flush=True)
  source_profile=json.loads(req('/api/info')[0])['default_id']
  saved=call('snapshot_create',label='source new group',services=['new'])['snapshot']
  assert saved['count']==3 and saved['profile_id']==source_profile
  source={'profile_id':source_profile,'snapshot_id':saved['id']}
  assert len(json.loads(call('snapshot_download',source=source)['content']))==3
  full=call('snapshot_create',label='source full',services=[])['snapshot']
  d1=call('compare',source=source,target={'profile_id':source_profile,'snapshot_id':full['id']},services=[])
  assert d1['counts']=={'unchanged':3}
  target=call('add_profile',name='target',url=f'http://127.0.0.1:{up.server_port}/target',username='admin')['profile']
  call('logout');token=call('login',username='admin',password='mock password',profile_id=target['id'])['session']
  assert not call('reports')['reports'];assert len(call('snapshots')['snapshots'])==2
  diff=call('compare',source=source,services=[])
  assert diff['counts']=={'changed':1,'missing_in_target':2,'target_only':1},diff['counts']
  assert next(x for x in diff['entries'] if x['ID']=='first')['changed_fields']==['name','port']
  assert not target_writes
  plan=call('sync_preview',source=source,services=[]);assert plan['counts']=={'conflict':1,'planned':2}
  call('apply',409,ticket=plan['ticket'],confirm=True);assert not target_writes
  plan=call('sync_preview',source=source,services=[])
  result=call('apply',ticket=plan['ticket'],confirm=True,skip_conflicts=True);assert result['exit_code']==0 and sorted(target_writes)==['fourth','second'] and target_state['first']['name']=='other' and 'extra' in target_state
  assert call('reports')['reports'][0]['source']==source
  print('PASS snapshot history, profile isolation, cross-environment diff and additive sync',flush=True)
  # Explicit login URL works even with a different selected profile ID.
  call('logout')
  before=json.loads(req('/api/info')[0])['profiles']
  call('login',400,username='admin',password='mock password',url='file:///tmp/x')
  call('login',401,username='admin',password='wrong',url=f'http://127.0.0.1:{up.server_port}/target/')
  assert json.loads(req('/api/info')[0])['profiles']==before
  res=call('login',username='admin',password='mock password',url=target['url']+'/',profile_id=source_profile)
  token=res['session'];assert res['profile']['id']==target['id'];assert len(json.loads(req('/api/info')[0])['profiles'])==len(before)
  call('logout')
  # A new URL spelling with a different but equivalent endpoint is a new saved profile.
  new_url=f'http://localhost:{up.server_port}/target'
  call('login',401,username='admin',password='wrong',url=new_url)
  assert json.loads(req('/api/info')[0])['profiles']==before
  res=call('login',username='admin',password='mock password',url=new_url,profile_id=source_profile)
  token=res['session'];assert res['profile']['url']==new_url and res['profile']['id'] not in (source_profile,target['id'])
  assert len(json.loads(req('/api/info')[0])['profiles'])==len(before)+1
  assert not call('reports')['reports']
  persisted=json.loads((pathlib.Path(d)/'profiles.json').read_text(encoding='utf-8'))
  assert all(set(p)=={'id','name','url','username'} for p in persisted)
  call('logout')
  print('PASS editable login URL, failed-login non-persistence and profile identity isolation',flush=True)
  if os.environ.get('TENSUNS_BROWSER_TEST')=='1':
   from playwright.sync_api import sync_playwright, expect
   with sync_playwright() as pw:
    browser=pw.chromium.launch()
    page=browser.new_page()
    errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
    page.goto(origin)
    expect(page.locator('#target')).to_have_value(f'http://127.0.0.1:{up.server_port}')
    expect(page.locator('#target')).to_be_editable()
    expect(page.locator('#username')).to_be_editable()
    page.locator('#target').fill(new_url)
    page.locator('#password').fill('mock password')
    page.locator('#loginForm button').click()
    expect(page.locator('#workspace')).to_be_visible(timeout=15000)
    expect(page.locator('#refresh')).to_be_enabled(timeout=15000)
    page.locator('[data-view="import"]').click()
    expect(page.locator('#batchCard')).to_be_visible()
    expect(page.locator('#singleCard')).to_be_hidden()
    page.locator('#importText').fill('new|batch-ui|192.0.2.1|9100||')
    page.locator('#importForm button[type=submit]').click()
    expect(page.locator('#previewCard')).to_be_visible()
    expect(page.locator('#confirm')).to_be_enabled()
    page.locator('#confirm').check()
    page.locator('[data-view="instances"]').click()
    page.locator('#addOne').click()
    expect(page.locator('#importHeading')).to_have_text('新增单个实例')
    expect(page.locator('#singleCard')).to_be_visible()
    expect(page.locator('#batchCard')).to_be_hidden()
    expect(page.locator('#previewCard')).to_be_hidden()
    expect(page.locator('#confirm')).not_to_be_checked()
    page.locator('#singleName').fill('new')
    page.locator('#singleId').fill('single-ui')
    page.locator('#singleAddress').fill('192.0.2.1')
    page.locator('#singleForm button[type=submit]').click()
    expect(page.locator('#previewCard')).to_be_visible()
    expect(page.locator('#confirm')).to_be_enabled()
    page.locator('#confirm').check()
    page.locator('[data-view="import"]').click()
    expect(page.locator('#importHeading')).to_have_text('批量导入')
    expect(page.locator('#batchCard')).to_be_visible()
    expect(page.locator('#singleCard')).to_be_hidden()
    expect(page.locator('#previewCard')).to_be_hidden()
    expect(page.locator('#confirm')).not_to_be_checked()
    expect(page.locator('#apply')).to_be_disabled()
    page.locator('[data-view="diff"]').click()
    expect(page.locator('#sourceSnapshot')).to_be_enabled(timeout=15000)
    page.locator('#sourceSnapshot').select_option(saved['id'])
    page.locator('#syncPreview').click()
    expect(page.locator('#importHeading')).to_have_text('跨环境新增同步')
    expect(page.locator('#batchCard')).to_be_hidden()
    expect(page.locator('#singleCard')).to_be_hidden()
    expect(page.locator('#previewCard')).to_be_visible()
    expect(page.locator('#backFromImport')).to_be_enabled()
    page.locator('#backFromImport').click()
    expect(page.locator('#view-diff')).to_be_visible()
    page.locator('[data-view="import"]').click()
    expect(page.locator('#batchCard')).to_be_visible()
    expect(page.locator('#previewCard')).to_be_hidden()
    assert not errors,errors
    assert 'batch-ui' not in target_writes and 'single-ui' not in target_writes
    browser.close()
   print('PASS real Chromium login URL and batch/single/sync UI isolation',flush=True)
 finally:
  p.terminate();p.wait(timeout=15)
up.shutdown()
