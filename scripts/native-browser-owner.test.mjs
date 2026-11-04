import test from 'node:test';
import assert from 'node:assert/strict';
import {nativeBrowserOwner} from './native-test.mjs';
import {connect} from './record-demo.mjs';

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
