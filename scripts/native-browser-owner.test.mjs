import test from 'node:test';
import assert from 'node:assert/strict';
import {nativeBrowserOwner} from './native-test.mjs';
import {connect} from './record-demo.mjs';
import {mkdtemp,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

test('each native browser has a stable distinct owner within its run',()=>{
  const owners=[0,1,2].map(index=>nativeBrowserOwner('unit-run',index));
  assert.equal(new Set(owners).size,3);
  assert.equal(nativeBrowserOwner('unit-run',1),owners[1]);
  assert.notEqual(nativeBrowserOwner('different-run',1),owners[1]);
  for(const [owner,index] of [[undefined,0],['',0],['run',-1],['run',3],['run',.5]])assert.throws(()=>nativeBrowserOwner(owner,index),/run owner and client index/);
});
test('invalid explicit browser owners fail before any connection or helper import',async()=>{
  for(const owner of [null,'',42])await assert.rejects(connect({owner,port:12345,target:'unit-only',url:'about:blank',helper:'/not-imported'}),/Browser owner/);
});
test('the real secondary whisper hook forwards its receipt owner, not the primary ambient owner',async t=>{
  const root=await mkdtemp(join(tmpdir(),'native-owner-')),previousTools=process.env.PI_BROWSER_TOOLS,previousSocket=globalThis.WebSocket,controller=new AbortController();
  t.after(async()=>{controller.abort();globalThis.WebSocket=previousSocket;if(previousTools===undefined)delete process.env.PI_BROWSER_TOOLS;else process.env.PI_BROWSER_TOOLS=previousTools;await rm(root,{recursive:true});});
  const primary=nativeBrowserOwner('unit-run',0),secondary=nativeBrowserOwner('unit-run',1),helper=join(root,'direct-cdp.mjs');
  await writeFile(helper,`export const owners=[];export const scriptOwner=()=>${JSON.stringify(primary)};export async function managedCDP(port,target,owner){owners.push(owner);return async()=>({});}`);
  process.env.PI_BROWSER_TOOLS=root;
  globalThis.WebSocket=class{constructor(){queueMicrotask(()=>this.onopen());}close(){this.onclose?.();}};
  t.mock.method(globalThis,'fetch',async()=>({json:async()=>[{type:'page',id:'unit-target',url:'about:blank',webSocketDebuggerUrl:'ws://unit-only'}]}));
  const {beforeNavigate}=await import('../examples/testing/whispers.mjs');
  await beforeNavigate({browser:{owner:secondary,port:12345,target:'unit-target'},signal:controller.signal});
  assert.deepEqual((await import(pathToFileURL(helper).href)).owners,[secondary]);
});
