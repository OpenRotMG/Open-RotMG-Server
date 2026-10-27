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
