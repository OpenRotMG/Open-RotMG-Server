import {test} from 'node:test';
import assert from 'node:assert/strict';
import {nativeClientArguments} from './native-test.mjs';

test('native client arguments preserve default/local modes and explicitly pin a manifest',()=>{
  assert.deepEqual(nativeClientArguments({}),[]);
  assert.deepEqual(nativeClientArguments({client:'/owned/client'}),['--client','/owned/client']);
  assert.deepEqual(nativeClientArguments({clientManifest:'/owned/selection.json'}),['--client-manifest','/owned/selection.json']);
  assert.throws(()=>nativeClientArguments({client:'/owned/client',clientManifest:'/owned/selection.json'}),/mutually exclusive/);
  assert.throws(()=>nativeClientArguments({clientManifest:'relative.json'}),/absolute manifest/);
});
