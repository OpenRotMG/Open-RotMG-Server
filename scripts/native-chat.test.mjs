import test from 'node:test';
import assert from 'node:assert/strict';
import {validateNativeChat,guardNativeInput,dispatchNativeChat} from './native-chat.mjs';

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
