import test from 'node:test';
import assert from 'node:assert/strict';
import {once} from 'node:events';
import {startChild,stopChild,browserDiagnostic,runCase} from './native-test.mjs';
import {mkdtemp,writeFile,readFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';

test('existing run directories are never reset',async()=>{
  const dir=await mkdtemp(join(tmpdir(),'native-test-existing-'));
  try{await writeFile(join(dir,'retained'),'original');await assert.rejects(runCase('/unused',{runDir:dir,browserTools:'/unused',chrome:'/unused',chromeArgs:[],timeoutMs:180000}),{code:'EEXIST'});assert.equal(await readFile(join(dir,'retained'),'utf8'),'original');}finally{await rm(dir,{recursive:true});}
});
test('an already-cancelled run cannot launch a child',()=>{
  assert.throws(()=>startChild(process.execPath,['-e','process.exit(0)'],{},AbortSignal.abort(Error('stopped'))),/stopped/);
});
test('cleanup escalates and reaps a child that ignores graceful signals',async()=>{
  const row=startChild(process.execPath,['-e','process.on("SIGINT",()=>{});process.on("SIGTERM",()=>{});console.log("ready");setInterval(()=>{},1000)'],{stdio:['ignore','pipe','pipe']},new AbortController().signal);
  try{await once(row.child.stdout,'data');const result=await stopChild(row,20);assert.equal(result.signal,'SIGKILL');assert.throws(()=>process.kill(row.child.pid,0),{code:'ESRCH'});}finally{if(!row.done)await stopChild(row,20);}
});
test('a failed spawn reports its actual launch error',async()=>{
  const row=startChild('/nonexistent/native-test-command',[],{},new AbortController().signal);
  await assert.rejects(row.started,{code:'ENOENT'});await row.closed;
});
test('browser-only diagnostics stay separate while active renderer errors fail',()=>{
  const p='[1:2:1020/120000.000001:ERROR:';
  assert.equal(browserDiagnostic(p+'google_apis/gcm/engine/registration_request.cc:291] Registration response error message: PHONE_REGISTRATION_ERROR'),'background-push');
  assert.equal(browserDiagnostic(p+'google_apis/gcm/engine/mcs_client.cc:702]   Error code: 401  Error message: Authentication Failed: wrong_secret'),'background-push');
  assert.equal(browserDiagnostic(p+'wrong.cc:291] Registration response error message: DEPRECATED_ENDPOINT'),'fatal');
  const gpu=p+'gpu/command_buffer/service/shared_image/shared_image_manager.cc:260] SharedImageManager::ProduceSkia: Trying to Produce a Skia representation from a non-existent mailbox.';
  assert.equal(browserDiagnostic(gpu),'fatal');assert.equal(browserDiagnostic(gpu,true),'renderer-shutdown');
  assert.equal(browserDiagnostic('thread worker panicked at failure',true),'fatal');
});
