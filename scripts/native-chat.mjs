// Fresh controller processes must use the original bootstrap facts, not adopt their first live sample.
export function nativeBootstrapIdentity(receipt, actors) {
  const fail=message=>{throw Error('Native bootstrap identity v1: '+message+'; matching runner/API/SDK bootstrap receipt required, no adoption or effect retry');};
  if(!receipt||receipt.version!==1||typeof receipt.runtime!=='string'||!receipt.runtime||!Array.isArray(receipt.actors)||Object.keys(receipt).sort().join(',')!=='actors,runtime,version')fail('missing or unsupported receipt');
  if(!Array.isArray(actors)||!actors.length||actors.length>8||actors.some(actor=>!Number.isSafeInteger(actor)||actor<=0)||new Set(actors).size!==actors.length)fail('configured registered actors must be unique');
  const expected=new Set(actors),identities=new Map();
  for(const row of receipt.actors){
    if(!row||Object.keys(row).sort().join(',')!=='account,actor,character,class'||!expected.has(row.actor)||identities.has(row.actor)||row.character!==row.actor||!Number.isSafeInteger(row.account)||row.account<=0||!Number.isSafeInteger(row.class)||row.class<=0||row.class>65535)fail('foreign, duplicate or malformed original tuple');
    identities.set(row.actor,Object.freeze({...row}));
  }
  if(identities.size!==expected.size)fail('configured original missing');
  return {runtime:receipt.runtime,identities};
}
export function captureNativeBootstrap(state, registered, previous) {
  const fail=message=>{throw Error('Native bootstrap identity v1: '+message+'; no new original identity was adopted');};
  if(!state||state.native_state_version!==1||typeof state.runtime!=='string'||!state.runtime||state.stopped!==null||!Array.isArray(state.actors))fail('unsupported atomic ready DTO');
  if(!Array.isArray(registered)||registered.some(row=>!row||!Number.isSafeInteger(row.account)||row.account<=0))fail('registered account missing');
  const actors=registered.map(row=>row.actor),players=new Set();
  const receipt={version:1,runtime:state.runtime,actors:state.actors.map(row=>{
    const owner=registered.find(r=>r.actor===row?.actor),o=row?.observation;
    if(!owner||row.stage!=='ready'||row.account!==owner.account||!Number.isSafeInteger(row.player)||row.player<=0||players.has(row.player)||!Number.isSafeInteger(row.world)||!Number.isSafeInteger(row.client_time)||typeof row.world_name!=='string'||!o||o.actor!==row.actor||o.controller!=='native'||o.session?.account!==row.account||o.session.character!==row.character||o.session.player!==row.player||o.session.world!==row.world||o.character?.class!==row.class||o.lifecycle?.alive!==true||o.lifecycle.death_pending!==false||o.lifecycle.quarantined!==false)fail('ready DTO differs from exact registered original/current session');
    players.add(row.player);
    return {actor:row.actor,account:row.account,character:row.character,class:row.class};
  })};
  const current=nativeBootstrapIdentity(receipt,actors);
  if(previous!==undefined){
    if(!previous||!Array.isArray(previous.actors))fail('unsupported prior receipt');
    const original=nativeBootstrapIdentity(previous,previous.actors.map(row=>row.actor));
    if(original.runtime!==current.runtime)fail('runtime epoch changed during enrollment');
    for(const [actor,identity] of original.identities){
      const row=current.identities.get(actor);
      if(!row||row.account!==identity.account||row.character!==identity.character||row.class!==identity.class)fail('prior original changed during enrollment');
    }
  }
  return receipt;
}

export function validateNativeChat(input) {
  if(!Object.hasOwn(input,'chat'))return;
  if(typeof input.chat!=='string'||input.chat.length<1||input.chat.length>120||!/^[\x20-\x7e]+$/.test(input.chat)||!input.chat.trim()||input.chat.includes('/'))throw Error('Native local chat requires 1–120 printable ASCII characters, nonblank, without slash commands or controls');
  if(!Array.isArray(input.keys)||input.keys.length||['mouse','drag','target'].some(k=>Object.hasOwn(input,k)))throw Error('Native local chat is exclusive of keys, mouse, drag and target');
}

// Check at the raw send boundary: managed CDP may wait before dispatching input.
export function guardNativeInput(method,params,state,leaseError,now=Date.now()) {
  if(method!=='Input.insertText'&&!(method.startsWith('Input.')&&['keyDown','mousePressed'].includes(params.type)))return;
  if(leaseError){state?.abort(leaseError);throw leaseError;}
  if(!state||state.stopped)throw Error('Native input outside its bounded hold');
  if(state.started&&now-state.started>=state.ms)throw Error('Native input missed its release deadline');
  if(!state.started)try{state.start(now);}catch(error){state.abort(error);throw error;}
}

export async function dispatchNativeChat(call,text) {
  const key=type=>call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
  const enter=async()=>{try{await key('keyDown');}finally{await key('keyUp');}};
  await enter();
  await call('Input.insertText',{text});
  await enter();
}

export async function holdNativeInput(inputHold,call,bindings,keys,mouse,ms,dragTo,chat,prepareDeadline) {
  if(inputHold.current)throw Error('Concurrent native hold is forbidden');
  if(!Number.isFinite(prepareDeadline)||prepareDeadline<=0)throw Error('Native hold requires a finite positive existing preparation deadline');
  const key=(type,k)=>call('Input.dispatchKeyEvent',{type,key:k,code:bindings[k][0],windowsVirtualKeyCode:bindings[k][1]});
  let resolveRelease,rejectRelease,timer,dragTimer,releasedAt,releasePromise,abortError;
  const released=new Promise((ok,bad)=>{resolveRelease=ok;rejectRelease=bad;});
  // A delayed drag rejection must reuse A's cleanup, never release a later hold's inputs.
  const release=()=>releasePromise??=(async()=>{state.stopped=true;releasedAt=Date.now();const results=await Promise.allSettled([...keys.map(k=>key('keyUp',k)),...(chat===undefined?[]:[call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13})]),...(mouse?[call('Input.dispatchMouseEvent',{type:'mouseReleased',...(dragTo??mouse),button:'left',buttons:0,clickCount:1})]:[])]);const errors=results.filter(r=>r.status==='rejected').map(r=>String(r.reason));if(errors.length)throw Error(errors.join('; '));})();
  const finish=()=>release().then(()=>abortError?rejectRelease(abortError):resolveRelease(),failure=>rejectRelease(abortError?Error(String(abortError)+'; input release unconfirmed: '+failure):failure));
  const state={ms,started:0,abort(error){abortError??=error;clearTimeout(timer);clearTimeout(dragTimer);return finish();},start(now=Date.now()){if(now>=prepareDeadline)throw Error('Native input missed its preparation deadline');this.started=now;timer=setTimeout(finish,ms);if(dragTo)dragTimer=setTimeout(()=>call('Input.dispatchMouseEvent',{type:'mouseMoved',...dragTo,button:'left',buttons:1}).catch(error=>state.abort(error)),ms/2);inputHold.started?.();}};
  inputHold.current=state;
  // Ruffle's map click establishes Flash focus before movement keys are delivered.
  const down=chat!==undefined?Promise.allSettled([dispatchNativeChat(call,chat).catch(async error=>{await state.abort(error);throw error;})]):(async()=>{const results=mouse?await Promise.allSettled([call('Input.dispatchMouseEvent',{type:'mousePressed',...mouse,button:'left',buttons:1,clickCount:1})]):[];if(results.every(r=>r.status==='fulfilled'))results.push(...await Promise.allSettled(keys.map(k=>key('keyDown',k))));return results;})();
  let noDispatch;
  try{if(!keys.length&&!mouse&&chat===undefined)state.start();
  noDispatch=setTimeout(()=>{if(!state.started)state.abort(Error('Native DOWN was not dispatched before its deadline'));},10000);
  await released;return {down,started:state.started,released:releasedAt};}finally{clearTimeout(timer);clearTimeout(dragTimer);clearTimeout(noDispatch);if(inputHold.current===state)inputHold.current=null;}
}
