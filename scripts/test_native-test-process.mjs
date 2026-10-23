import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {EventEmitter} from 'node:events';
import vm from 'node:vm';

const source=readFileSync(new URL('./native-test-process.mjs',import.meta.url),'utf8');
const runner=readFileSync(new URL('./native-test.mjs',import.meta.url),'utf8');
const body=source.replace(/^import .*;\n/gm,'').replace(/^export /gm,'').replace(/^if\(process\.argv\[1\].*$/m,'');
const fault=code=>Object.assign(Error(code),{code});
function fixture() {
  const calls=[],signals=[],writes=new Map(),rows=new Map(),child=new EventEmitter();child.pid=42;
  const token={pid:42,uid:1000,start:'123'};
  rows.set(42,{uids:[1000,1000,1000,1000],start:'123',group:42,session:42,state:'S',inodeUid:0});
  let readHook,killHook,delayHook;
  const read=path=>{
    calls.push(path);if(readHook)readHook(path);
    if(path==='/proc/sys/kernel/random/boot_id')return 'fake-boot';
    if(writes.has(path))return writes.get(path);
    const m=/^\/proc\/(\d+)\/(status|stat)$/.exec(path);assert(m,'unexpected fake read: '+path);
    const pid=Number(m[1]),row=rows.get(pid);if(!row)throw fault('ENOENT');
    if(m[2]==='status')return row.status??`State:\t${row.state} (fake)\nUid:\t${row.uids.join('\t')}\n`;
    const fields=Array(20).fill('0');fields[0]=row.state;fields[2]=String(row.group);fields[3]=String(row.session);fields[19]=row.start;
    return `${pid} (fake child) ${fields.join(' ')}`;
  };
  const context={Error,AggregateError,readFileSync:read,accessSync:path=>{calls.push(path);assert(/^\/proc\/\d+$/.test(path));if(!rows.has(Number(path.slice(6))))throw fault('ENOENT');},
    statSync:()=>{calls.push('inode');return {uid:0};},spawn:()=>child,writeFileSync:(path,data)=>writes.set(path,data),renameSync:(from,to)=>{writes.set(to,writes.get(from));writes.delete(from);},join:(...parts)=>parts.join('/'),
    process:{getuid:()=>1000,kill:(pid,signal)=>{signals.push([pid,signal]);if(killHook)killHook(pid,signal);}},Date:{now:()=>100},
    delay:async ms=>{if(delayHook)await delayHook(ms);}};
  const api=vm.runInNewContext(body+'\n({identity,sameProcess,startChild,stopChild,guardRun})',context);
  const start=()=>api.startChild('fake',[],{},new AbortController().signal);
  const ready=async()=>{const row=start();child.emit('spawn');await row.started;return row;};
  return {api,context,token,child,rows,calls,signals,writes,start,ready,onRead:f=>readHook=f,onKill:f=>killHook=f,onDelay:f=>delayHook=f};
}

test('identity: four kernel UIDs precede metadata, ignoring nondumpable inode UID0',()=>{
  const f=fixture(),actual=f.api.identity(42);
  assert.equal(actual.uid,1000);assert.equal(actual.start,'123');
  assert.deepEqual(f.calls,['/proc/42/status','/proc/42/stat']);assert.equal(f.rows.get(42).inodeUid,0);
});
test('identity: each of all four UID mismatches fails before stat',()=>{
  for(let index=0;index<4;index++){const f=fixture();f.rows.get(42).uids[index]=0;assert.throws(()=>f.api.identity(42),/kernel UID mismatch/);assert.deepEqual(f.calls,['/proc/42/status']);}
});
test('identity: missing or malformed UID credentials cannot authorize metadata',()=>{
  for(const status of ['Name:\tfake\n','Uid:\t1000 1000 1000\n','Uid:\t1000 1000 nope 1000\n']){
    const f=fixture();f.rows.get(42).status=status;assert.throws(()=>f.api.identity(42),/invalid credentials/);assert.deepEqual(f.calls,['/proc/42/status']);
  }
});
test('identity: absence is reproved and live PID with missing metadata is fatal',()=>{
  const f=fixture();f.rows.delete(42);assert.equal(f.api.identity(42),null);assert.deepEqual(f.calls,['/proc/42/status','/proc/42']);
  const g=fixture();g.onRead(()=>{throw fault('ENOENT');});assert.throws(()=>g.api.identity(42),/exists without readable metadata/);
});
test('identity: validated zombie credentials prove dead without start metadata',()=>{
  const f=fixture();f.rows.get(42).state='Z';assert.equal(f.api.identity(42),null);assert.deepEqual(f.calls,['/proc/42/status']);
});
test('startChild: caller holds actual row before bootstrap metadata rejection; cleanup never adopts',async()=>{
  const f=fixture(),error=fault('EACCES');let held;
  f.onRead(()=>{assert.equal(held.child,f.child);throw error;});held=f.start();assert.equal(held.identity,null);assert.equal(f.calls.length,0);
  f.child.emit('spawn');await assert.rejects(held.started,e=>e===error);await assert.rejects(f.api.stopChild(held),e=>e===error);assert.equal(held.identity,null);assert.deepEqual(f.signals,[]);
});
test('stopChild: live creation-token mismatch never signals',async()=>{
  const f=fixture(),row=await f.ready();f.rows.get(42).start='124';await assert.rejects(f.api.stopChild(row),/creation token mismatch/);assert.deepEqual(f.signals,[]);
});
test('stopChild: detached group or session mismatch never signals',async()=>{
  for(const field of ['group','session']){const f=fixture(),row=await f.ready();f.rows.get(42)[field]=99;await assert.rejects(f.api.stopChild(row),/group\/session mismatch/);assert.deepEqual(f.signals,[]);}
});
test('stopChild: ESRCH accepts only reproved absent or dead child and actual close',async()=>{
  for(const state of ['absent','Z']){
    const f=fixture(),row=await f.ready();f.onKill(()=>{if(state==='absent')f.rows.delete(42);else f.rows.get(42).state='Z';f.child.emit('close',0,null);throw fault('ESRCH');});
    const result=await f.api.stopChild(row);assert.equal(result.code,0);assert.equal(row.done,true);assert.deepEqual(f.signals,[[-42,'SIGINT']]);
  }
});
test('stopChild: ESRCH with live or unprovable PID retains signal and proof causes',async()=>{
  for(const mode of ['live','metadata','reused']){
    const f=fixture(),row=await f.ready(),error=fault('ESRCH');
    f.onKill(()=>{if(mode==='metadata')f.onRead(()=>{throw fault('EACCES');});if(mode==='reused')f.rows.get(42).start='124';throw error;});
    await assert.rejects(f.api.stopChild(row),e=>e instanceof AggregateError&&e.cause===error&&e.errors.length===2&&/absence\/death proof/.test(e.message));assert.deepEqual(f.signals,[[-42,'SIGINT']]);assert.equal(row.done,false);
  }
});
test('stopChild: every escalation revalidates the original token',async()=>{
  const f=fixture(),row=await f.ready();f.onDelay(()=>{f.rows.get(42).start='124';});await assert.rejects(f.api.stopChild(row,0),/creation token mismatch/);assert.deepEqual(f.signals,[[-42,'SIGINT']]);
});
test('guardian: mismatched child is never signaled, other owned child cleanup still runs',async()=>{
  const f=fixture();f.writes.set('/qa/children.json',JSON.stringify({boot:'fake-boot',parent:{pid:9,uid:1000,start:'1'},deadline:1000,done:false,children:[{...f.token,start:'122'},{pid:43,uid:1000,start:'123'}]}));f.writes.set('/qa/result.json','{"status":"running"}');
  f.rows.set(43,{...f.rows.get(42),group:43,session:43});f.onKill(pid=>f.rows.delete(-pid));
  await assert.rejects(f.api.guardRun('/qa'),/Guardian cleanup failed/);assert.deepEqual(f.signals,[[-43,'SIGINT']]);const result=JSON.parse(f.writes.get('/qa/guardian.json'));assert.equal(result.remaining[0].pid,42);assert(result.errors.length>0);
});
test('launch registration: row and rejection handlers precede throwing registry persistence',async()=>{
  const f=fixture(),children=[],failures=[],error=Error('registry write failed');
  const launchBody=runner.slice(runner.indexOf('  const launch='),runner.indexOf('  const waitFor='));
  const launch=vm.runInNewContext(launchBody+'\nlaunch',{check:()=>{},startChild:f.api.startChild,root:'/qa',controller:new AbortController(),children,register:()=>{throw error;},fail:e=>failures.push(e),process:{env:{}},Error});
  assert.throws(()=>launch('fake','fake',[],[]),e=>e===error);assert.equal(children[0].child,f.child);f.child.emit('spawn');await children[0].started;await Promise.resolve();await Promise.resolve();assert(failures.includes(error));
});
test('cleanup: unknown bootstrap PID stays explicit and unsuccessful while other cleanup continues',async()=>{
  const start=runner.indexOf('    for(const row of children.reverse())'),end=runner.indexOf('    if(guardian)try{',start),error=Error('bootstrap metadata denied'),causes=[],report={children:[],cleanupErrors:[]},registryState={done:false};
  const children=[{name:'unknown',child:{pid:42},identity:null},{name:'known',child:{pid:43},identity:{pid:43,uid:1000,start:'123'}}];
  await vm.runInNewContext('(async()=>{'+runner.slice(start,end)+'})()',{children,report,registryState,register:()=>{},cleanupError:e=>{causes.push(e);report.cleanupErrors.push(String(e));},stopChild:async row=>{if(row.identity===null)throw error;return {pid:row.child.pid,code:0,signal:null};},tabs:[]});
  assert.equal(causes[0],error);assert.equal(registryState.done,false);assert.equal(report.children.find(row=>row.pid===42).identity,null);assert.match(report.children.find(row=>row.pid===42).error,/metadata denied/);assert.equal(report.children.find(row=>row.pid===43).code,0);
});
test('guardian: persistence failure retains causes without skipping known cleanup',async()=>{
  const f=fixture(),error=fault('ENOSPC');f.writes.set('/qa/children.json',JSON.stringify({boot:'fake-boot',parent:{pid:9,uid:1000,start:'1'},deadline:1000,done:false,children:[f.token]}));
  f.context.writeFileSync=()=>{throw error;};f.onKill(pid=>f.rows.delete(-pid));
  await assert.rejects(f.api.guardRun('/qa'),e=>e instanceof AggregateError&&e.errors.includes(error));assert.deepEqual(f.signals,[[-42,'SIGINT']]);
});
test('guardian wait: original 5000ms expiry remains failure even if checked escalation closes code0',async()=>{
  const start=runner.indexOf('    if(guardian)try{'),end=runner.indexOf('    if(registryState&&report.cleanupErrors.length)',start),errors=[],report={children:[]},waits=[],stops=[];
  const guardian={child:{pid:42},identity:{pid:42,uid:1000,start:'123'},closed:new Promise(()=>{})};
  await vm.runInNewContext('(async()=>{'+runner.slice(start,end)+'})()',{guardian,report,cleanupError:e=>errors.push(e),delay:async ms=>waits.push(ms),stopChild:async(row,grace)=>{stops.push([row,grace]);return {pid:42,code:0,signal:null};},assert,Error});
  assert.deepEqual(waits,[5000]);assert.equal(stops[0][0],guardian);assert.equal(stops[0][1],0);assert.match(errors[0].message,/exceeded original 5000ms/);assert.equal(report.children[0].code,0);
});
