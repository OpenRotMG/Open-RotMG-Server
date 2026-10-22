import test from 'node:test';
import assert from 'node:assert/strict';
import {hasNativeEntry,nativeEntry} from './native-test.mjs';

function entryFixture(module){
  const abort=new AbortController(),calls={automatic:0};
  const stopped=new Promise((_,reject)=>abort.signal.addEventListener('abort',()=>reject(abort.signal.reason),{once:true}));stopped.catch(()=>{});
  const browser={owner:'native-test:synthetic',port:9222,target:'synthetic-target',pid:123,notForwarded(){assert.fail('capability escaped');}};
  const client={actor:3,account:4,browser,connection:{call(){assert.fail('CDP escaped');}},api(){assert.fail('API escaped');}};
  const enter=()=>nativeEntry(module,client,{signal:abort.signal,stopped,check:()=>abort.signal.throwIfAborted()},async()=>{calls.automatic++;});
  return{abort,calls,client,enter};
}

test('native entry absent own export invokes legacy exactly once',async()=>{
  for(const module of [{},Object.create({enter(){assert.fail('inherited hook invoked');}})]){
    const f=entryFixture(module);assert.equal(hasNativeEntry(module),false);await f.enter();assert.equal(f.calls.automatic,1);
  }
});
test('native entry explicit undefined null or nonfunction never selects legacy',async()=>{
  for(const value of [undefined,null,false,1,'enter',{},Promise.resolve()]){
    const module={enter:value},f=entryFixture(module);
    assert.throws(()=>hasNativeEntry(module),/enter export must be a function/);
    await assert.rejects(f.enter(),/enter export must be a function/);assert.equal(f.calls.automatic,0);
  }
});
test('native entry receives only frozen exact identities browser data and existing signal',async()=>{
  let observed;
  const module={enter(context){assert.equal(this,module);observed=context;}};const f=entryFixture(module);await f.enter();
  assert.deepEqual(Object.keys(observed).sort(),['account','actor','browser','signal']);
  assert.equal(observed.actor,3);assert.equal(observed.account,4);assert.equal(observed.signal,f.abort.signal);
  assert.deepEqual(observed.browser,{owner:'native-test:synthetic',port:9222,target:'synthetic-target',pid:123});
  assert(Object.isFrozen(observed)&&Object.isFrozen(observed.browser));assert.notEqual(observed.browser,f.client.browser);
  assert.throws(()=>{observed.actor=9;},TypeError);assert.throws(()=>{observed.browser.port=1;},TypeError);
  f.client.browser.owner='changed outside';assert.equal(observed.browser.owner,'native-test:synthetic');
  f.abort.abort(Error('signal remains live'));assert.equal(observed.signal.aborted,true);assert.equal(f.calls.automatic,0);
});
test('native entry rejects capability-bearing identities and invalid managed data before hook',async()=>{
  for(const change of [f=>f.client.account=()=>{},f=>f.client.actor={},f=>f.client.browser.target={},f=>f.client.browser.pid=0,f=>f.client.browser.port=0]){
    let invoked=0;const f=entryFixture({enter(){invoked++;}});change(f);await assert.rejects(f.enter(),/Entry requires/);assert.equal(invoked,0);assert.equal(f.calls.automatic,0);
  }
});
test('native entry awaits hook completion and suppresses automatic menu input',async()=>{
  const held=Promise.withResolvers(),entered=Promise.withResolvers();let completed=false;
  const f=entryFixture({async enter(){entered.resolve();await held.promise;}});
  const pending=f.enter().then(()=>{completed=true;});await entered.promise;assert.equal(completed,false);assert.equal(f.calls.automatic,0);
  held.resolve();await pending;assert.equal(completed,true);assert.equal(f.calls.automatic,0);
});
test('native entry sync and async failures retain the original error without legacy',async()=>{
  const reason=Error('synthetic observed entry failure');
  for(const enter of [()=>{throw reason;},async()=>{throw reason;}]){
    const f=entryFixture({enter});await assert.rejects(f.enter(),error=>error===reason);assert.equal(f.calls.automatic,0);
  }
});
test('native entry already aborted or aborted before hook dispatch performs no input',async()=>{
  for(const before of [true,false]){
    let invoked=0;const reason=Error('entry cancelled');const f=entryFixture({enter(){invoked++;}});
    if(before)f.abort.abort(reason);const pending=f.enter();if(!before)f.abort.abort(reason);
    await assert.rejects(pending,error=>error===reason);assert.equal(invoked,0);assert.equal(f.calls.automatic,0);
  }
});
test('native entry abort unwinds outer cleanup despite a noncooperating hook',async()=>{
  const entered=Promise.withResolvers(),reason=Error('existing active deadline'),f=entryFixture({enter(context){assert.equal(context.signal,f.abort.signal);entered.resolve();return new Promise(()=>{});}});
  let cleaned=false;const pending=(async()=>{try{await f.enter();}finally{cleaned=true;}})();
  await entered.promise;f.abort.abort(reason);await assert.rejects(pending,error=>error===reason);
  assert.equal(cleaned,true);assert.equal(f.calls.automatic,0);
});
