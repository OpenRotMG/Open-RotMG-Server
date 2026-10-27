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
  if(!state.started)state.start();
}

export async function dispatchNativeChat(call,text) {
  const key=type=>call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
  const enter=async()=>{try{await key('keyDown');}finally{await key('keyUp');}};
  await enter();
  await call('Input.insertText',{text});
  await enter();
}

export async function holdNativeInput(inputHold,call,bindings,keys,mouse,ms,dragTo,chat) {
  if(inputHold.current)throw Error('Concurrent native hold is forbidden');
  const key=(type,k)=>call('Input.dispatchKeyEvent',{type,key:k,code:bindings[k][0],windowsVirtualKeyCode:bindings[k][1]});
  let resolveRelease,rejectRelease,timer,dragTimer,releasedAt,releasePromise,abortError;
  const released=new Promise((ok,bad)=>{resolveRelease=ok;rejectRelease=bad;});
  // A delayed drag rejection must reuse A's cleanup, never release a later hold's inputs.
  const release=()=>releasePromise??=(async()=>{state.stopped=true;releasedAt=Date.now();const results=await Promise.allSettled([...keys.map(k=>key('keyUp',k)),...(chat===undefined?[]:[call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13})]),...(mouse?[call('Input.dispatchMouseEvent',{type:'mouseReleased',...(dragTo??mouse),button:'left',buttons:0,clickCount:1})]:[])]);const errors=results.filter(r=>r.status==='rejected').map(r=>String(r.reason));if(errors.length)throw Error(errors.join('; '));})();
  const finish=()=>release().then(()=>abortError?rejectRelease(abortError):resolveRelease(),failure=>rejectRelease(abortError?Error(String(abortError)+'; input release unconfirmed: '+failure):failure));
  const state={ms,started:0,abort(error){abortError??=error;clearTimeout(timer);clearTimeout(dragTimer);return finish();},start(){this.started=Date.now();timer=setTimeout(finish,ms);if(dragTo)dragTimer=setTimeout(()=>call('Input.dispatchMouseEvent',{type:'mouseMoved',...dragTo,button:'left',buttons:1}).catch(error=>state.abort(error)),ms/2);}};
  inputHold.current=state;
  // Ruffle's map click establishes Flash focus before movement keys are delivered.
  const down=chat!==undefined?Promise.allSettled([dispatchNativeChat(call,chat).catch(async error=>{await state.abort(error);throw error;})]):(async()=>{const results=mouse?await Promise.allSettled([call('Input.dispatchMouseEvent',{type:'mousePressed',...mouse,button:'left',buttons:1,clickCount:1})]):[];if(results.every(r=>r.status==='fulfilled'))results.push(...await Promise.allSettled(keys.map(k=>key('keyDown',k))));return results;})();
  if(!keys.length&&!mouse&&chat===undefined)state.start();
  const noDispatch=setTimeout(()=>{if(!state.started)state.abort(Error('Native DOWN was not dispatched before its deadline'));},10000);
  try{await released;return {down,started:state.started,released:releasedAt};}finally{clearTimeout(timer);clearTimeout(dragTimer);clearTimeout(noDispatch);if(inputHold.current===state)inputHold.current=null;}
}
