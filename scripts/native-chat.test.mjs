import test from 'node:test';
import assert from 'node:assert/strict';
import {validateNativeChat,guardNativeInput,dispatchNativeChat,holdNativeInput} from './native-chat.mjs';

const input=chat=>({keys:[],duration_ms:1000,chat});
function controller(beforeSend=()=>{}) {
  const events=[],attempts=[];
  let now=100,leaseError;
  const state={ms:1000,started:0,stopped:false,start(){this.started=now;},abort(){this.stopped=true;}};
  const call=async(method,params)=>{
    attempts.push({method,...params});
    await beforeSend(method,params);
    guardNativeInput(method,params,state,leaseError,now);
    events.push({method,...params});
  };
  return {call,events,attempts,state,advance(ms){now+=ms;},loseLease(){leaseError=Error('lease lost');},stop(){state.abort();}};
}
const enter=type=>({method:'Input.dispatchKeyEvent',type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
const flush=()=>new Promise(resolve=>setImmediate(resolve));
function lifecycle(beforeSend=()=>{},afterSend=()=>{}) {
  const inputHold={current:null},events=[],bindings={w:['KeyW',87]};
  const call=async(method,params)=>{
    await beforeSend(method,params);
    guardNativeInput(method,params,inputHold.current);
    events.push({method,...params});
    await afterSend(method,params);
  };
  return {inputHold,events,hold:(...args)=>holdNativeInput(inputHold,call,bindings,...args)};
}

test('native-chat refuses invalid or mixed input before any effects',async()=>{
  const c=controller();
  const invalid=[undefined,null,1,'','   ','/help',' hello /tell','hello\nthere','hello\r','hello\t','hello\x00','hello\x7f','héllo','x'.repeat(121)];
  const mixed=[{keys:['w']},{mouse:null},{drag:{}},{target:{entity:1}}];
  for(const value of [...invalid.map(input),...mixed.map(fields=>({...input('hello'),...fields}))]){
    await assert.rejects(async()=>{validateNativeChat(value);await dispatchNativeChat(c.call,value.chat);});
  }
  assert.deepEqual(c.attempts,[]);
  for(const text of ['snake here',' x ', 'x'.repeat(120),'Hello! @#$%^&*()_+-=[]{};:"\',.<>?`~\\'])assert.doesNotThrow(()=>validateNativeChat(input(text)));
});

test('native-chat preserves ordinary nonchat input and raw key/mouse holds',async()=>{
  for(const ordinary of [{keys:['w'],duration_ms:600},{keys:[],mouse:{x:1,y:2}},{keys:[],drag:{from:{x:1,y:2},to:{x:3,y:4}}},{keys:['q'],target:{entity:1}}]){
    const before=structuredClone(ordinary);validateNativeChat(ordinary);assert.deepEqual(ordinary,before);
  }
  for(const [method,params] of [['Input.dispatchKeyEvent',{type:'keyDown',key:'w'}],['Input.dispatchMouseEvent',{type:'mousePressed',x:1,y:2}]]){
    const c=controller();await c.call(method,params);assert.equal(c.state.started,100);assert.deepEqual(c.events,[{method,...params}]);
  }
});

test('native-chat opens, types unmodified text and submits exactly once in order',async()=>{
  const c=controller(),text='snake here';validateNativeChat(input(text));
  await dispatchNativeChat(c.call,text);
  assert.deepEqual(c.events,[enter('keyDown'),enter('keyUp'),{method:'Input.insertText',text},enter('keyDown'),enter('keyUp')]);
});

test('native-chat releases Enter even when either DOWN or UP fails; never retries',async()=>{
  for(let failure=0;failure<5;failure++){
    let index=0;
    const c=controller(()=>{if(index++===failure)throw Error('injected CDP failure');});
    await assert.rejects(dispatchNativeChat(c.call,'hello'),/injected CDP failure/);
    assert.equal(c.attempts.length,failure===0?2:failure===3?5:failure+1);
    if(failure===0||failure===3)assert.deepEqual(c.attempts.at(-1),enter('keyUp'));
    assert(c.attempts.filter(e=>e.type==='keyDown').length<=2);
  }
});

for(const after of ['keyDown','insertText'])for(const stop of ['deadline','abort','lease']){
  test(`native-chat blocks late text/submit after ${stop} following ${after}`,async()=>{
    let c;
    c=controller((method,params)=>{
      // Delay the managed call before its raw dispatch, as a real receipt check can.
      if((after==='keyDown'&&method==='Input.insertText')||(after==='insertText'&&params.type==='keyDown'&&c.events.some(e=>e.method==='Input.insertText'))){
        if(stop==='deadline')c.advance(1000);else if(stop==='lease')c.loseLease();else c.stop();
      }
    });
    await assert.rejects(dispatchNativeChat(c.call,'hello'),/deadline|bounded hold|lease lost/);
    assert.equal(c.events.filter(e=>e.type==='keyDown').length,1);
    assert.equal(c.events.filter(e=>e.method==='Input.insertText').length,after==='insertText'?1:0);
    assert.deepEqual(c.events.at(-1),enter('keyUp'));
  });
}

test('native-chat refuses abort or lease loss before opening and still releases Enter',async()=>{
  for(const stop of ['stop','loseLease']){
    const c=controller();c[stop]();
    await assert.rejects(dispatchNativeChat(c.call,'hello'),/bounded hold|lease lost/);
    assert.deepEqual(c.events,[enter('keyUp')]);
  }
});

test('native-chat cannot open or type outside an active hold, including after cleanup',()=>{
  for(const state of [undefined,{stopped:true,started:100,ms:1000}])for(const [method,params] of [['Input.insertText',{text:'hello'}],['Input.dispatchKeyEvent',{type:'keyDown'}]]){
    assert.throws(()=>guardNativeInput(method,params,state,undefined,100),/bounded hold/);
  }
  assert.doesNotThrow(()=>guardNativeInput('Input.dispatchKeyEvent',{type:'keyUp'},undefined,Error('lease lost')));
});

for(const next of ['chat','key'])test(`holdNativeInput late drag rejection cannot stop or release B's pending ${next} input`,async t=>{
  t.mock.timers.enable({apis:['setTimeout','Date'],now:100});
  const drag=Promise.withResolvers(),pendingDown=Promise.withResolvers();
  let delayB=false;
  const c=lifecycle(async(method,params)=>{
    if(delayB&&params.type==='keyDown')await pendingDown.promise;
  },async(method,params)=>{
    if(params.type==='mouseMoved')await drag.promise;
  });
  const a=c.hold(['w'],{x:1,y:2},100,{x:3,y:4});
  await flush();t.mock.timers.tick(50);await flush();
  assert(c.events.some(e=>e.type==='mouseMoved'));
  t.mock.timers.tick(50);
  const resultA=await a;await resultA.down;
  assert.equal(c.inputHold.current,null);
  assert.equal(resultA.started,100);assert.equal(resultA.released,200);
  const releasedA=c.events.slice();
  delayB=true;
  const b=next==='chat'?c.hold([],undefined,100,undefined,'B remains active'):c.hold(['w'],undefined,100);
  const stateB=c.inputHold.current;
  drag.reject(Error('late A drag failure'));await flush();
  assert.equal(c.inputHold.current,stateB);
  assert.notEqual(stateB.stopped,true);
  assert.deepEqual(c.events,releasedA,'A must not resend keyUp or mouseReleased into B');
  pendingDown.resolve();await flush();
  assert.equal(stateB.started,200);
  assert.deepEqual(c.events.slice(releasedA.length),next==='chat'?
    [enter('keyDown'),enter('keyUp'),{method:'Input.insertText',text:'B remains active'},enter('keyDown'),enter('keyUp')]:
    [{method:'Input.dispatchKeyEvent',type:'keyDown',key:'w',code:'KeyW',windowsVirtualKeyCode:87}]);
  t.mock.timers.tick(100);
  const resultB=await b;
  assert((await resultB.down).every(r=>r.status==='fulfilled'));
  assert.equal(c.inputHold.current,null);
});

test('holdNativeInput late drag rejection with no active hold reuses completed cleanup',async t=>{
  t.mock.timers.enable({apis:['setTimeout','Date'],now:100});
  const drag=Promise.withResolvers();
  const c=lifecycle(undefined,async(method,params)=>{if(params.type==='mouseMoved')await drag.promise;});
  const a=c.hold([],{x:1,y:2},100,{x:3,y:4});
  await flush();t.mock.timers.tick(50);await flush();t.mock.timers.tick(50);
  const result=await a;await result.down;
  const events=c.events.slice();
  drag.reject(Error('late A drag failure'));await flush();
  assert.equal(c.inputHold.current,null);
  assert.deepEqual(c.events,events);
});

test('holdNativeInput confirms in-flight cleanup before rejecting a current abort',async t=>{
  t.mock.timers.enable({apis:['setTimeout','Date'],now:100});
  const release=Promise.withResolvers();
  const c=lifecycle(undefined,async(method,params)=>{if(params.type==='keyUp')await release.promise;});
  let settled=false;
  const hold=c.hold(['w'],undefined,100);
  const rejection=assert.rejects(hold,/current hold failed/).then(()=>{settled=true;});
  await flush();t.mock.timers.tick(100);await flush();
  const state=c.inputHold.current;
  const aborted=state.abort(Error('current hold failed'));
  await flush();assert.equal(settled,false);assert.equal(c.inputHold.current,state);
  assert.equal(c.events.filter(e=>e.type==='keyUp').length,1);
  release.resolve();await aborted;await rejection;
  assert.equal(c.inputHold.current,null);
});

test('holdNativeInput reports chat failure only after confirmed release, including release failure',async t=>{
  t.mock.timers.enable({apis:['setTimeout','Date'],now:100});
  const release=Promise.withResolvers();
  let keyUps=0,settled=false;
  const c=lifecycle(undefined,async(method,params)=>{
    if(method==='Input.insertText')throw Error('chat dispatch failed');
    if(params.type==='keyUp'&&++keyUps===2)await release.promise;
  });
  const hold=c.hold([],undefined,100,undefined,'hello');
  const rejection=assert.rejects(hold,/chat dispatch failed; input release unconfirmed: .*release failed/).then(()=>{settled=true;});
  await flush();const state=c.inputHold.current;
  assert.equal(state.stopped,true);assert.equal(settled,false);
  release.reject(Error('release failed'));await rejection;
  assert.equal(c.inputHold.current,null);
  assert.equal(c.events.filter(e=>e.type==='keyDown').length,1);
});

test('holdNativeInput confirms cleanup on undispatched DOWN timeout and blocks the delayed DOWN',async t=>{
  t.mock.timers.enable({apis:['setTimeout','Date'],now:100});
  const down=Promise.withResolvers(),release=Promise.withResolvers();
  const c=lifecycle(async(method,params)=>{if(params.type==='keyDown')await down.promise;},
    async(method,params)=>{if(params.type==='keyUp')await release.promise;});
  let settled=false;
  const hold=c.hold(['w'],undefined,100);
  const rejection=assert.rejects(hold,/DOWN was not dispatched/).then(()=>{settled=true;});
  await flush();t.mock.timers.tick(10000);await flush();
  assert.equal(c.inputHold.current.stopped,true);assert.equal(settled,false);
  down.resolve();await flush();
  assert.deepEqual(c.events,[{method:'Input.dispatchKeyEvent',type:'keyUp',key:'w',code:'KeyW',windowsVirtualKeyCode:87}]);
  release.resolve();await rejection;assert.equal(c.inputHold.current,null);
});
