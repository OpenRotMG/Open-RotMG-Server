import test from 'node:test';
import assert from 'node:assert/strict';
import {earnedLedger,collisionMap,route,localTargets,originalIdentity,textReceiver,assertCastleArrival,structuredEvents,mapInput} from '../examples/testing/realm-earned-core.mjs';
const roots=Array.from({length:49},(_,i)=>i+1),census={message:'finite Realm closure roster bound',origin:-3,heroes:49};
const credit=(hero,remaining,now=100)=>({message:'qualifying Realm hero defeated',origin:-3,hero,remaining,now});
const waiting=source=>({message:'quest defeat earned a pending Realm event',source}),admission=(source,entity)=>({message:'quest defeat summoned a Realm event',source,entity});
test('structured events accept launcher preamble and known ready lines but reject unknown runtime output',()=>{
  const parse=structuredEvents('http://127.0.0.1:12345');assert.equal(parse('Local source: abc dirty=False'),null);assert.equal(parse('    Finished `release` profile [optimized]'),null);assert.deepEqual(parse(JSON.stringify({fields:census})),census);assert.equal(parse('Local server ready. Open: http://127.0.0.1:12345/'),null);assert.equal(parse('Logs: the terminal and .local/server.log'),null);assert.equal(parse(''),null);assert.throws(()=>parse('thread panicked at runtime'),/Unexpected unstructured/);assert.throws(()=>parse('{bad'));assert.throws(()=>parse('{}'),/Invalid structured/);
});
test('late ledger binding consumes the owned census history exactly once',()=>{
  const l=earnedLedger(-3,roots,[census,credit(1,48)]);assert(l.snapshot().census);assert.deepEqual(l.snapshot().credited,[1]);assert.throws(()=>l.ingest(census),/duplicate/);assert.throws(()=>earnedLedger(-3,roots,undefined),/Explicit owned/);
});
test('earned ledger requires all49 plus every admitted root and at least one event',()=>{
  const l=earnedLedger(-3,roots,[]);l.ingest(census);l.ingest(waiting(1));l.ingest(admission(1,50));
  for(let id=1;id<=50;id++)l.ingest(credit(id,50-id,id));
  const s=l.complete();assert.equal(s.credited.length,50);assert.deepEqual(s.admitted,[50]);assert.deepEqual(s.pending,[]);
  const empty=earnedLedger(-3,roots,[]);empty.ingest(census);for(let id=1;id<=49;id++)empty.ingest(credit(id,49-id,id));assert.throws(()=>empty.complete(),/No earned event/);
});
test('ledger rejects unsettled, duplicate, invented and clockless credits',()=>{
  for(const bad of [{...credit(1,48),now:undefined},credit(99,48),{...credit(1,48),now:null}]){const l=earnedLedger(-3,roots,[]);l.ingest(census);assert.throws(()=>l.ingest(bad),/Credit requires/);}
  const l=earnedLedger(-3,roots,[]);l.ingest(census);l.ingest(waiting(1));assert.throws(()=>l.ingest(credit(1,48)),/Credit requires/);assert.throws(()=>l.ingest(admission(2,50)),/Unowned/);l.ingest(admission(1,50));l.ingest(credit(1,49));assert.throws(()=>l.ingest(credit(1,48)),/Credit requires/);assert.throws(()=>l.ingest(credit(2,48,99)),/Credit requires/);
});
test('roster scope and exact coordinator remaining count cannot be substituted',()=>{
  assert.throws(()=>earnedLedger(-3,roots.slice(1),[census]),/mismatched normal census/);const l=earnedLedger(-3,roots,[]);assert.throws(()=>l.ingest(credit(1,48)),/Credit requires/);l.ingest(census);l.ingest({...credit(1,48),origin:7});assert.equal(l.snapshot().credited.length,0);assert.throws(()=>l.ingest(credit(1,47)),/ledgers differ/);
});
test('current normal roster is bound to its actual census rather than an obsolete49 constant',()=>{
  const current=Array.from({length:56},(_,i)=>i+1),l=earnedLedger(-3,current,[{...census,heroes:56}]);l.ingest(waiting(1));l.ingest(admission(1,57));for(let id=1;id<=57;id++)l.ingest(credit(id,57-id,id));assert.equal(l.complete().initial.length,56);assert.equal(l.complete().credited.length,57);assert.throws(()=>earnedLedger(-3,current,[census]),/mismatched/);
});
test('selected baseline and reconstruction layouts require exact catalogued inputs',()=>{
  const catalogue=[{kind:'baseline',id:'realm-fsod-1',path:'realm-fsod-1.wmap',sha256:'a'.repeat(64)},{kind:'reconstruction',recipe:'world06.json',sha256:'b'.repeat(64)}];assert.deepEqual(mapInput('realm-fsod-1',catalogue),{file:'data/realm-fsod-1.wmap',sha256:'a'.repeat(64)});assert.deepEqual(mapInput('classic-world-06-reconstruction',catalogue),{recipe:'world06.json',sha256:'b'.repeat(64)});assert.throws(()=>mapInput('unknown',catalogue),/explicit catalogued/);assert.throws(()=>mapInput('classic-world-07-reconstruction',catalogue),/must be catalogued/);
});
test('route respects collision and rejects a genuinely unreachable encounter',()=>{
  const map={width:21,height:21,blocked:new Uint8Array(441)};for(let y=0;y<21;y++)if(y!==18)map.blocked[y*21+10]=1;
  const points=route(map,{x:2.5,y:10.5},{x:18.5,y:10.5});assert(points.some(p=>p.y===18.5));assert(points.every(p=>!map.blocked[Math.floor(p.y)*21+Math.floor(p.x)]));map.blocked[18*21+10]=1;assert.throws(()=>route(map,{x:2.5,y:10.5},{x:18.5,y:10.5}),/No collision-legal/);
});
test('actual runtime hazards and solids prohibit navigation and invalid projections fail',()=>{
  const live={width:3,height:3,blocked:[[1,1]],damaging_ground:[[2,0]]},m=collisionMap(live);assert.equal(m.blocked[2],1);assert.equal(m.blocked[4],1);assert.equal(m.blocked[0],0);assert.throws(()=>collisionMap({...live,damaging_ground:undefined}),/Actual runtime/);assert.throws(()=>collisionMap({...live,blocked:[[-1,0]]}),/outside map/);assert.throws(()=>collisionMap({...live,width:0}),/dimensions/);
  assert.equal(collisionMap({...live,blocked:[],damaging_ground:[]}).blocked[2],0);
});
test('visible combat targets retain protected phases and prefer a real unprotected family member',()=>{
  const objects=[{id:1,hp:10,position:{x:1,y:1},condition_mask:1<<23},{id:2,hp:100,position:{x:2,y:1},condition_mask:0}],family=[{id:1},{id:2}];assert.equal(localTargets(objects,family,{x:0,y:0})[0].id,2);assert.equal(localTargets(objects.slice(0,1),family,{x:0,y:0})[0].id,1);assert.throws(()=>localTargets([{...objects[0],condition_mask:undefined}],family,{x:0,y:0}),/int32/);
});
test('original cohort identity rejects a mismatched class or controller',()=>{
  const spec={class:0x310,level:20,stats:[670,385,50,25,50,50,40,75],equipment:[0xaf6,0xb25,0xb05,0xac5],backpack:Array(8).fill(-1)},v={actor:1,controller:'native',lifecycle:{alive:true,death_pending:false,quarantined:false},session:{account:2,character:3},character:{class:0x310,level:20,hp:670,mp:385,base_stats:[...spec.stats],items:[...spec.equipment,...spec.backpack],inventory_revision:4}};assert.equal(originalIdentity('fighter',spec,v).character,3);assert.throws(()=>originalIdentity('fighter',spec,{...v,controller:'game_bot'}));assert.throws(()=>originalIdentity('fighter',spec,{...v,character:{...v.character,class:0x30e}}));
});
test('Castle arrival uses the actual nested inspect view and original living identity',()=>{
  const original={world:'Realm',session:{account:2,character:3}},view={map:{name:"Oryx's Castle"},session:{account:2,character:3},lifecycle:{alive:true}};
  assertCastleArrival(view,original);assert.throws(()=>assertCastleArrival({...view,world:"Oryx's Castle",map:{name:'Realm'}},original));assert.throws(()=>assertCastleArrival({...view,session:{account:2,character:4}},original));assert.throws(()=>assertCastleArrival({...view,lifecycle:{alive:false}},original));
});
test('actual TEXT framing preserves RC4 across fragments and consumes cleanText',()=>{
  const utf=s=>{const b=Buffer.from(s),n=Buffer.alloc(2);n.writeUInt16BE(b.length);return Buffer.concat([n,b]);},body=Buffer.concat([utf('#Oryx the Mad God'),Buffer.alloc(9),utf(''),utf('I HAVE CLOSED THIS REALM!'),utf('')]);
  const key=Buffer.from('72c5583cafb6818995cbd74b80','hex'),s=Array.from({length:256},(_,i)=>i);let j=0,a=0;for(let i=0;i<256;i++){j=(j+s[i]+key[i%key.length])&255;[s[i],s[j]]=[s[j],s[i]];}j=0;
  const frames=[];for(let n=0;n<2;n++){const b=Buffer.from(body);for(let i=0;i<b.length;i++){a=(a+1)&255;j=(j+s[a])&255;[s[a],s[j]]=[s[j],s[a]];b[i]^=s[(s[a]+s[j])&255];}const h=Buffer.alloc(5);h.writeInt32BE(5+b.length);h[4]=25;frames.push(h,b);}
  const data=Buffer.concat(frames),r=textReceiver(),feed=payload=>r.event({method:'Network.webSocketFrameReceived',params:{requestId:'one-owned-connection',response:{opcode:2,payloadData:payload.toString('base64')}}});feed(data.subarray(0,3));feed(data.subarray(3,19));feed(data.subarray(19));assert.equal(r.error,null);assert.equal(r.texts.length,2);assert.equal(r.texts[0].name,'#Oryx the Mad God');assert.equal(r.texts[1].cleanText,'');
  const bad=Buffer.alloc(5);bad.writeInt32BE(4);feed(bad);assert(r.error);
});
