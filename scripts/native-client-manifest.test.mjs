import {test} from 'node:test';
import assert from 'node:assert/strict';
import {nativeClientArguments,nativeRealmArguments} from './native-test.mjs';

test('native Realm selection preserves zero and rejects invalid seed fixtures',()=>{
  assert.deepEqual(nativeRealmArguments(undefined),[]);
  assert.deepEqual(nativeRealmArguments(0),['--realm-seed','0']);
  assert.deepEqual(nativeRealmArguments(9009),['--realm-seed','9009']);
  for(const seed of [null,false,'0',-1,0.5,NaN,Infinity,Number.MAX_SAFE_INTEGER+1])assert.throws(()=>nativeRealmArguments(seed),/nonnegative safe integer/);
});

test('native client arguments preserve default/local modes and explicitly pin a manifest',()=>{
  assert.deepEqual(nativeClientArguments({}),[]);
  assert.deepEqual(nativeClientArguments({client:'/owned/client'}),['--client','/owned/client']);
  assert.deepEqual(nativeClientArguments({clientManifest:'/owned/selection.json'}),['--client-manifest','/owned/selection.json']);
  assert.throws(()=>nativeClientArguments({client:'/owned/client',clientManifest:'/owned/selection.json'}),/mutually exclusive/);
  assert.throws(()=>nativeClientArguments({clientManifest:'relative.json'}),/absolute manifest/);
});
