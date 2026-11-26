'use strict';
const $=id=>document.getElementById(id);
let profiles=[],activeProfile=null,progressTimer=null,snapshotList=[],diffResult=null,diffPage=1;
let currentView='instances',importMode='batch';
let session='',busy=false,groups=[],page=1,total=0,preview=null,previewPage=1,lastReport='';
const statusNames={planned:'计划新增',unchanged:'配置一致',conflict:'配置冲突'};
let messageTimer;
function message(s,error=false){clearTimeout(messageTimer);$('message').textContent=s;$('message').className=error?'error':'';$('message').hidden=false;if(!busy)messageTimer=setTimeout(()=>{$('message').hidden=true},8000)}
function setBusy(v){busy=v;document.querySelectorAll('button,input,select,textarea').forEach(e=>{if(v){e.dataset.previousDisabled=e.disabled?'1':'0';e.disabled=true}else if('previousDisabled' in e.dataset){e.disabled=e.dataset.previousDisabled==='1';delete e.dataset.previousDisabled}});if(!v)updateApply()}
async function task(fn){if(busy)return;setBusy(true);message('正在处理，请勿重复提交…');try{await fn();if($('message').textContent==='正在处理，请勿重复提交…')$('message').hidden=true}catch(e){message(e.message||String(e),true)}finally{setBusy(false);if(!$('message').hidden){clearTimeout(messageTimer);messageTimer=setTimeout(()=>{$('message').hidden=true},10000)}}}
async function call(action){const headers={'Content-Type':'application/json'};if(session)headers['X-TenSunS-Session']=session;
 const r=await fetch('/api/action',{method:'POST',headers,body:JSON.stringify(action),cache:'no-store',credentials:'same-origin'});
 const raw=await r.text();let d;try{d=JSON.parse(raw)}catch{d={error:raw.slice(0,300)||'响应不是 JSON'}}
 if(r.status===401&&action.action!=='login'){reset();throw Error('会话已失效，请重新登录')}
 if(!r.ok)throw Error('HTTP '+r.status+'：'+(d.error||'操作失败'));return d}
function reset(){currentView='instances';importMode='batch';session='';activeProfile=null;snapshotList=[];diffResult=null;$('diffResult').hidden=true;['snapshotsBody','diffBody','diffCounts','sourceSnapshot','targetSnapshot','snapshotGroups','diffGroups'].forEach(id=>$(id).replaceChildren());groups=[];page=1;total=0;preview=null;lastReport='';$('workspace').hidden=true;$('login').hidden=false;$('logout').hidden=true;$('connection').textContent='未连接';$('password').value='';$('importText').value='';$('importFile').value='';$('query').value='';$('singleName').value='';$('singleId').value='';$('singleAddress').value='';$('singleTags').value='[]';$('singleMeta').value='{}';['instancesBody','previewBody','reportsBody','exportGroups','group'].forEach(id=>$(id).replaceChildren());$('previewCard').hidden=true;$('resultCard').hidden=true;$('singleCard').hidden=true;$('batchCard').hidden=false;$('groupCount').textContent='—';$('instanceCount').textContent='—';$('matchCount').textContent='—'}
function show(view,mode='batch'){
 // Every import entry starts a new UI flow. No stale preview, confirmation or result
 // can carry from single-instance, batch or cross-environment sync to another flow.
 if(currentView==='import'||view==='import'){
  invalidate();lastReport='';$('resultCard').hidden=true;$('previewBody').replaceChildren();$('resultSummary').textContent='';
 }
 if(view==='import'){
  importMode=mode;
  $('batchCard').hidden=mode!=='batch';$('singleCard').hidden=mode!=='single';
  $('importHeading').textContent=mode==='single'?'新增单个实例':mode==='sync'?'跨环境新增同步':'批量导入';
  $('importDescription').textContent=mode==='single'?'填写单个实例配置，预览并确认后新增。':mode==='sync'?'检查同步来源、目标及导入计划。':'先校验并预览，明确确认后才写入 TenSunS。';
  $('backFromImport').hidden=mode==='batch';
  if(mode==='single')$('singleForm').reset();
 }
 currentView=view;
 document.querySelectorAll('.content>section').forEach(x=>x.hidden=x.id!=='view-'+view);
 document.querySelectorAll('.nav').forEach(x=>x.classList.toggle('active',x.dataset.view===(view==='import'&&mode==='single'?'instances':view==='import'&&mode==='sync'?'diff':view)))
}
function cell(tr,value,cls){const td=document.createElement('td');td.textContent=value==null?'':String(value);if(cls)td.className=cls;tr.append(td);return td}
function row(body,values){const tr=document.createElement('tr');values.forEach(x=>cell(tr,x));body.append(tr);return tr}
function download(filename,content,encoding){if(encoding==='base64')content=Uint8Array.from(atob(content),c=>c.charCodeAt(0));const u=URL.createObjectURL(new Blob([content],{type:'application/octet-stream'}));const a=document.createElement('a');a.href=u;a.download=filename;document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(u),2000)}
async function loadGroups(){const d=await call({action:'groups'});groups=d.groups;const old=$('group').value;const selected=new Set(Array.from($('exportGroups').querySelectorAll('input:checked'),e=>e.value));$('group').replaceChildren(new Option('全部服务组',''));$('exportGroups').replaceChildren();for(const g of groups){$('group').append(new Option(g.name+' ('+g.count+')',g.name));const label=document.createElement('label'),c=document.createElement('input');c.type='checkbox';c.value=g.name;c.checked=selected.has(g.name);label.append(c,document.createTextNode(g.name+' ('+g.count+')'));$('exportGroups').append(label)}if(groups.some(g=>g.name===old))$('group').value=old;$('groupCount').textContent=groups.length.toLocaleString();$('instanceCount').textContent=groups.reduce((s,g)=>s+g.count,0).toLocaleString()}
async function loadInstances(){const size=Number($('pageSize').value),query=$('query').value,service=$('group').value||null;let d=await call({action:'instances',service,query,page,page_size:size});total=d.total;const pages=Math.max(1,Math.ceil(total/size));if(page>pages){page=pages;d=await call({action:'instances',service,query,page,page_size:size});total=d.total}
 const body=$('instancesBody');body.replaceChildren();for(const r of d.rows)row(body,[r.name,r.ID,r.address,r.port,JSON.stringify(r.tags),JSON.stringify(r.meta)]);
 if(!d.rows.length){const tr=row(body,['没有匹配的实例']);tr.firstChild.colSpan=6}
 $('matchCount').textContent=total.toLocaleString();$('pageInfo').textContent='共 '+total.toLocaleString()+' 条 · '+page+' / '+Math.max(1,Math.ceil(total/size))+' 页';setPager('previous',page<=1);setPager('next',page*size>=total)}
function setPager(id,disabled){const e=$(id);if(busy){e.dataset.previousDisabled=disabled?'1':'0';e.disabled=true}else e.disabled=disabled}
function updateApply(){const conflicts=preview?.counts?.conflict||0;const planned=preview?.counts?.planned||0;$('apply').disabled=busy||!preview||!$('confirm').checked||planned===0||(conflicts>0&&!$('skip').checked)}
function invalidate(){preview=null;$('previewCard').hidden=true;$('confirm').checked=false;$('skip').checked=false;updateApply()}
function renderPreview(){const items=preview.entries;const size=50;const body=$('previewBody');body.replaceChildren();for(const entry of items.slice((previewPage-1)*size,previewPage*size)){const tr=document.createElement('tr');cell(tr,statusNames[entry.status]||entry.status,'status-'+entry.status);for(const v of [entry.instance.name,entry.instance.ID,entry.instance.address,entry.instance.port,entry.existing_groups.join(', ')])cell(tr,v);body.append(tr)}$('previewPage').textContent=previewPage+' / '+Math.max(1,Math.ceil(items.length/size));setPager('previewPrev',previewPage<=1);setPager('previewNext',previewPage*size>=items.length)}
function showPreview(d,description=''){preview=d;previewPage=1;for(const [id,k] of [['planned','planned'],['unchanged','unchanged'],['conflicts','conflict']])$(id).textContent=preview.counts[k]||0;$('previewInfo').textContent=(description?description+' · ':'')+'共 '+preview.entries.length+' 条，预览有效期 10 分钟。执行前重新核对远端配置。';$('previewCard').hidden=false;renderPreview();updateApply();message('预览完成，尚未写入远端')}
async function makePreview(format,content){invalidate();$('resultCard').hidden=true;showPreview(await call({action:'preview',format,content}))}
async function loadReports(){const d=await call({action:'reports'});const body=$('reportsBody');body.replaceChildren();for(const x of d.reports){const tr=row(body,[x.id,dateText(x.created),x.state,x.source?.snapshot_id||'—',x.exit_code??'—']);const b=document.createElement('button');b.textContent='下载 JSONL';b.addEventListener('click',()=>task(async()=>{const r=await call({action:'report',id:x.id});download(r.filename,r.content)}));cell(tr,'').append(b)}if(!d.reports.length){const tr=row(body,['本会话暂无报告']);tr.firstChild.colSpan=6}}
$('loginForm').addEventListener('submit',e=>{e.preventDefault();task(async()=>{let password=$('password').value;$('password').value='';try{const d=await call({action:'login',username:$('username').value.trim(),password,url:$('target').value.trim(),profile_id:$('profileSelect').value});session=d.session;activeProfile=d.profile}finally{password=''}$('login').hidden=true;$('workspace').hidden=false;$('logout').hidden=false;$('connection').textContent='已连接 · '+activeProfile.name;show('instances');await loadProfiles(activeProfile.id);await loadGroups();await loadInstances();message('TenSunS 登录成功')})});
$('logout').addEventListener('click',()=>task(async()=>{try{await call({action:'logout'})}finally{reset()}}));
document.querySelectorAll('.nav').forEach(e=>e.addEventListener('click',()=>task(async()=>{show(e.dataset.view);if(e.dataset.view==='reports')await loadReports();if(e.dataset.view==='profiles')await loadProfiles();if(['snapshots','diff'].includes(e.dataset.view))await loadSnapshots()})));
$('refresh').onclick=()=>task(async()=>{page=1;await loadGroups();await loadInstances()});
$('searchForm').onsubmit=e=>{e.preventDefault();task(async()=>{page=1;await loadInstances()})};
$('group').onchange=$('pageSize').onchange=()=>task(async()=>{page=1;await loadInstances()});
$('previous').onclick=()=>task(async()=>{if(page>1)page--;await loadInstances()});
$('next').onclick=()=>task(async()=>{if(page*Number($('pageSize').value)<total)page++;await loadInstances()});
$('addOne').onclick=()=>{if(busy)return;show('import','single');$('singleName').value=$('group').value;$('singleId').focus()};
$('backFromImport').onclick=()=>{if(!busy)show(importMode==='sync'?'diff':'instances')};
$('importFile').onchange=()=>{invalidate();const f=$('importFile').files[0];if(f){const ext=f.name.split('.').pop().toLowerCase();if(['txt','csv','json','jsonl','xlsx'].includes(ext))$('importFormat').value=ext}};
$('importText').oninput=$('importFormat').onchange=invalidate;
$('singleForm').addEventListener('input',invalidate);
$('importForm').onsubmit=e=>{e.preventDefault();task(async()=>{const f=$('importFile').files[0];if(f&&f.size>8*1024*1024)throw Error('文件最大 8 MiB（浏览器上传限制）');let content;if($('importFormat').value==='xlsx'){if(!f)throw Error('请选择 XLSX 文件');const bytes=new Uint8Array(await f.arrayBuffer());let binary='';for(let i=0;i<bytes.length;i+=8192)binary+=String.fromCharCode(...bytes.subarray(i,i+8192));content=btoa(binary)}else content=f?await f.text():$('importText').value;if(!content.trim())throw Error('请选择文件或输入配置');if(content.length>12*1024*1024)throw Error('配置内容过大');await makePreview($('importFormat').value,content)})};
$('singleForm').onsubmit=e=>{e.preventDefault();task(async()=>{const r={ID:$('singleId').value.trim(),name:$('singleName').value.trim(),address:$('singleAddress').value.trim(),port:Number($('singlePort').value),tags:JSON.parse($('singleTags').value||'[]'),meta:JSON.parse($('singleMeta').value||'{}')};await makePreview('json',JSON.stringify([r]))})};
$('template').onclick=()=>{const f=$('importFormat').value;if(f==='xlsx'){task(async()=>{const d=await call({action:'template'});download(d.filename,d.content,d.encoding)});return;}const r={ID:'switch-01',name:'snmp_exporter',address:'192.0.2.11',port:161,tags:[],meta:{vendor:'huawei'}};const samples={txt:'# ServiceName|InstanceID|IP|Port|Tags|Meta\nsnmp_exporter|switch-01|192.0.2.11|161||vendor=huawei\n',csv:'ServiceName,InstanceID,IP,Port,Tags,Meta\nsnmp_exporter,switch-01,192.0.2.11,161,[],"{""vendor"":""huawei""}"\n',json:JSON.stringify([r],null,2)+'\n',jsonl:JSON.stringify(r)+'\n'};download('instances.'+f,samples[f])};
$('confirm').onchange=$('skip').onchange=updateApply;
$('previewPrev').onclick=()=>{if(previewPage>1){previewPage--;renderPreview()}};
$('previewNext').onclick=()=>{if(preview&&previewPage*50<preview.entries.length){previewPage++;renderPreview()}};
$('apply').onclick=()=>task(async()=>{if(!preview||!$('confirm').checked)return;const ticket=preview.ticket,skip=$('skip').checked;invalidate();let d;progressTimer=setInterval(async()=>{try{const r=await fetch('/api/progress',{headers:{'X-TenSunS-Session':session},cache:'no-store'});const x=await r.json();if(x.running)message('执行中 · '+Object.entries(x.counts).map(([k,n])=>k+': '+n).join(' / '))}catch{}},1000);try{d=await call({action:'apply',ticket,confirm:true,skip_conflicts:skip})}finally{clearInterval(progressTimer)}lastReport=d.report||'';$('resultCard').hidden=false;$('resultSummary').textContent=(d.exit_code===0?'执行完成':'执行失败或结果未完全验证，请先核对报告')+' · '+Object.entries(d.counts).map(([k,n])=>k+': '+n).join(' / ')+(d.message?' · '+d.message:'');setPager('downloadResult',!lastReport);message(d.exit_code===0?'执行完成，请查看报告':'存在失败或未验证结果，请查看报告',d.exit_code!==0);await loadGroups()});
$('downloadResult').onclick=()=>{if(lastReport)download('import-result.jsonl',lastReport)};
$('exportRefresh').onclick=()=>task(loadGroups);
$('selectAll').onclick=()=>{$('exportGroups').querySelectorAll('input').forEach(x=>x.checked=true)};
$('clearSelection').onclick=()=>{$('exportGroups').querySelectorAll('input').forEach(x=>x.checked=false)};
$('exportForm').onsubmit=e=>{e.preventDefault();task(async()=>{const services=Array.from($('exportGroups').querySelectorAll('input:checked'),x=>x.value);const d=await call({action:'export',format:$('exportFormat').value,services});download(d.filename,d.content,d.encoding);message('已导出 '+d.count+' 条实例')})};
$('reportRefresh').onclick=()=>task(loadReports);
async function loadProfiles(preferred){const r=await fetch('/api/info',{cache:'no-store'});if(!r.ok)throw Error('无法读取连接配置');const d=await r.json();profiles=d.profiles;const sel=$('profileSelect'),old=preferred||sel.value;sel.replaceChildren(...profiles.map(p=>new Option(p.name+' · '+p.url,p.id)));sel.value=profiles.some(p=>p.id===old)?old:d.default_id;renderProfile();$('version').textContent='V'+d.version;$('profilesBody').replaceChildren();for(const p of profiles)row($('profilesBody'),[p.name,p.url,p.username])}
function renderProfile(){const p=profiles.find(p=>p.id===$('profileSelect').value);if(p){$('target').value=p.url;$('username').value=p.username;$('password').value=''}}
$('profileSelect').onchange=renderProfile;
$('target').oninput=$('username').oninput=()=>{$('password').value=''};
$('profileForm').onsubmit=e=>{e.preventDefault();task(async()=>{await call({action:'add_profile',name:$('profileName').value,url:$('profileUrl').value,username:$('profileUser').value});e.target.reset();await loadProfiles();message('连接已保存，退出后可切换')})};
function dateText(n){return new Date(n*1000).toLocaleString()}
function snapshotRef(value){if(!value)return null;const x=snapshotList.find(s=>s.id===value);if(!x)throw Error('快照不存在，请刷新');return {profile_id:x.profile_id,snapshot_id:x.id}}
function selected(id){return Array.from($(id).selectedOptions,o=>o.value)}
function snapshotTitle(s){return s.profile_name+' / '+s.label+' / '+dateText(s.created)+' / '+s.count+' 条'}
function refreshDiffScope(){const s=snapshotList.find(x=>x.id===$('sourceSnapshot').value);$('diffGroups').replaceChildren(...(s?.services||[]).map(g=>new Option(g,g)));$('syncNotice').textContent='新增同步目标：'+(activeProfile?.name||'—')+' ('+(activeProfile?.url||'—')+')。同步不会覆盖冲突、不会删除目标独有实例。';$('diffResult').hidden=true;diffResult=null}
async function loadSnapshots(){
 const d=await call({action:'snapshots'});snapshotList=d.snapshots;
 const body=$('snapshotsBody');body.replaceChildren();for(const s of snapshotList){const tr=row(body,[s.label,s.profile_name,s.all_groups?'全量':s.services.join(', '),s.count,dateText(s.created)]);const b=document.createElement('button');b.textContent='下载 JSON';b.onclick=()=>task(async()=>{const r=await call({action:'snapshot_download',source:snapshotRef(s.id)});download(r.filename,r.content)});cell(tr,'').append(b)}
 if(!snapshotList.length){const tr=row(body,['暂无快照']);tr.firstChild.colSpan=6}
 $('snapshotGroups').replaceChildren(...groups.map(g=>new Option(g.name+' ('+g.count+')',g.name)));
 const source=$('sourceSnapshot'),target=$('targetSnapshot'),s0=source.value,t0=target.value;
 source.replaceChildren(new Option('选择来源快照',''),...snapshotList.map(s=>new Option(snapshotTitle(s),s.id)));
 target.replaceChildren(new Option('当前连接（实时）',''),...snapshotList.filter(s=>s.all_groups).map(s=>new Option(snapshotTitle(s),s.id)));
 if(snapshotList.some(s=>s.id===s0))source.value=s0;
 if(snapshotList.some(s=>s.id===t0&&s.all_groups))target.value=t0;refreshDiffScope()
}
$('snapshotRefresh').onclick=$('diffRefresh').onclick=()=>task(async()=>{await loadGroups();await loadSnapshots()});
$('snapshotForm').onsubmit=e=>{e.preventDefault();task(async()=>{await call({action:'snapshot_create',label:$('snapshotLabel').value,services:selected('snapshotGroups')});$('snapshotLabel').value='';await loadSnapshots();message('快照已保存')})};
$('sourceSnapshot').onchange=refreshDiffScope;
$('targetSnapshot').onchange=$('diffGroups').onchange=()=>{diffResult=null;$('diffResult').hidden=true};
const diffNames={missing_in_target:'目标缺失',changed:'字段差异',target_only:'目标独有',ambiguous:'ID 歧义',unchanged:'配置一致'};
function renderDiff(){
 if(!diffResult)return;const filter=$('diffFilter').value,items=diffResult.entries.filter(e=>!filter||e.status===filter),size=50,pages=Math.max(1,Math.ceil(items.length/size));diffPage=Math.min(diffPage,pages);
 const body=$('diffBody');body.replaceChildren();for(const e of items.slice((diffPage-1)*size,diffPage*size)){const tr=document.createElement('tr');cell(tr,e.ID);cell(tr,diffNames[e.status]||e.status,e.status==='missing_in_target'?'status-planned':e.status==='unchanged'?'status-unchanged':'status-conflict');cell(tr,e.changed_fields.join(', '));cell(tr,JSON.stringify(e.source,null,2));cell(tr,JSON.stringify(e.target,null,2));body.append(tr)}
 $('diffPage').textContent=items.length+' 条 · '+diffPage+' / '+pages;setPager('diffPrev',diffPage<=1);setPager('diffNext',diffPage>=pages)
}
$('diffForm').onsubmit=e=>{e.preventDefault();task(async()=>{const source=snapshotRef($('sourceSnapshot').value);if(!source)throw Error('请选择来源快照');diffResult=await call({action:'compare',source,target:snapshotRef($('targetSnapshot').value),services:selected('diffGroups')});diffPage=1;$('diffFilter').value='';$('diffCounts').replaceChildren();for(const [k,v] of Object.entries(diffNames)){const box=document.createElement('div'),label=document.createElement('small'),n=document.createElement('strong');label.textContent=v;n.textContent=diffResult.counts[k]||0;box.append(label,n);$('diffCounts').append(box)}$('diffResult').hidden=false;renderDiff();message('差异比较完成，未写入远端')})};
$('diffFilter').onchange=()=>{diffPage=1;renderDiff()};$('diffPrev').onclick=()=>{if(diffPage>1)diffPage--;renderDiff()};$('diffNext').onclick=()=>{diffPage++;renderDiff()};$('downloadDiff').onclick=()=>{if(diffResult)download('tensuns-diff.json',JSON.stringify(diffResult,null,2))};
$('syncPreview').onclick=()=>task(async()=>{if($('targetSnapshot').value)throw Error('新增同步仅能写入当前连接，比较目标请选择当前连接');const source=snapshotRef($('sourceSnapshot').value);if(!source)throw Error('请选择来源快照');const label=snapshotTitle(snapshotList.find(s=>s.id===source.snapshot_id));invalidate();$('resultCard').hidden=true;const d=await call({action:'sync_preview',source,services:selected('diffGroups')});show('import','sync');showPreview(d,'来源：'+label+' → 当前目标：'+activeProfile.name+' ('+activeProfile.url+')')});
loadProfiles().catch(e=>{message(e.message,true);$('target').placeholder='请输入 TenSunS Web URL'});
