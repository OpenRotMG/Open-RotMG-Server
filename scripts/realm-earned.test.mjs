import test from 'node:test';
import assert from 'node:assert/strict';
import {earnedLedger,collisionMap,route,localTargets,originalIdentity,textReceiver,assertCastleArrival,structuredEvents,mapInput,readMap,recoveryState,replacementEntrance,otherEntrance,observedAim,inputWork,nativeReceiptRoom,partyPositions} from '../examples/testing/realm-earned-core.mjs';
const roots=Array.from({length:49},(_,i)=>i+1),census={message:'finite Realm closure roster bound',origin:-3,heroes:49};
const credit=(hero,remaining,now=100)=>({message:'qualifying Realm hero defeated',origin:-3,hero,remaining,now});
const waiting=source=>({message:'quest defeat earned a pending Realm event',source}),admission=(source,entity)=>({message:'quest defeat summoned a Realm event',source,entity});
test('structured events accept launcher preamble and known ready lines but reject unknown runtime output',()=>{
  const parse=structuredEvents('http://127.0.0.1:12345');assert.equal(parse('Local source: abc dirty=False'),null);assert.equal(parse('    Finished `release` profile [optimized]'),null);assert.deepEqual(parse(JSON.stringify({fields:census})),census);assert.equal(parse('Local server ready. Open: http://127.0.0.1:12345/'),null);assert.equal(parse('Logs: the terminal and .local/server.log'),null);assert.equal(parse(''),null);assert.throws(()=>parse('thread panicked at runtime'),/Unexpected unstructured/);assert.throws(()=>parse('{bad'));assert.throws(()=>parse('{}'),/Invalid structured/);
});
test('Nexus discovery explicitly reads legacy zero-HP portals while combat uses the summary',async()=>{
  const entities=[{id:10,kind:0x712,hp:0},{id:11,kind:0x712,hp:0},{id:12,kind:0x604,hp:100}],seen=[],headers={Authorization:'Bearer own-fixture'};
  const request=async(url,options)=>{const query=new URL(url).searchParams;assert.equal(query.get('player'),'7');assert.equal(options.headers,headers);const summary=query.get('entity_summary')==='true';seen.push(summary);return {ok:true,json:async()=>summary?{projection:'entity_summary',entities:entities.filter(e=>e.hp>0)}:{entities}};};
  const nexus=await readMap('http://127.0.0.1:12345',headers,7,false,request);assert.deepEqual(nexus.entities.filter(e=>e.kind===0x712).map(e=>e.id),[10,11]);
  const combat=await readMap('http://127.0.0.1:12345',headers,7,true,request);assert.deepEqual(combat.entities.map(e=>e.id),[12]);assert.deepEqual(seen,[false,true]);
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
test('held aim binds a live observed point without claiming the enemy survives resume',()=>{
  const view={position:{x:1,y:1},objects:[{id:4,hp:100,position:{x:2,y:1}}]};assert.deepEqual(observedAim(view,4,0),{x:350,y:325});const aim=observedAim(view,4,-Math.PI/4);assert(aim.x>=0&&aim.x<600&&aim.y>=0&&aim.y<600);view.objects[0].hp=0;assert.throws(()=>observedAim(view,4,0),/living enemy/);
  assert.throws(()=>observedAim(view,9,0),/observed neighborhood/);view.objects[0].hp=100;view.objects[0].position.x=NaN;assert.throws(()=>observedAim(view,4,0),/finite/);
});
test('visible combat targets retain protected phases and prefer a real unprotected family member',()=>{
  const objects=[{id:1,hp:10,position:{x:1,y:1},condition_mask:1<<23},{id:2,hp:100,position:{x:2,y:1},condition_mask:0}],family=[{id:1},{id:2}];assert.equal(localTargets(objects,family,{x:0,y:0})[0].id,2);assert.equal(localTargets(objects.slice(0,1),family,{x:0,y:0})[0].id,1);assert.throws(()=>localTargets([{...objects[0],condition_mask:undefined}],family,{x:0,y:0}),/int32/);
});
test('native sequences account for every planned hold and the entire bounded duration',()=>{
  const input={keys:[],duration_ms:500},batch=Array.from({length:12},()=>({actor:1,input:{...input,chat:'DrFlux follow me'}}));assert.deepEqual(inputWork(batch,1),{duration:6000,inputs:12});assert.deepEqual(inputWork(input,1),{duration:500,inputs:1});
  for(const bad of [[],Array(33).fill(batch[0]),[{actor:2,input}],[{actor:1,input:{...input,duration_ms:NaN}}],[{actor:1,input:{...input,duration_ms:0}}],Array(21).fill(batch[0])])assert.throws(()=>inputWork(bad,1));assert.throws(()=>inputWork({...input,duration_ms:10001},1));
});
test('native client receipts include bootstrap and cannot be confused with backend receipts',()=>{
  const files=[...Array.from({length:256},(_,i)=>i+'.client.json'),...Array.from({length:524},(_,i)=>i+'.receipt.json')];assert.throws(()=>nativeReceiptRoom(files,240,0),/native client receipt reserve/);
  const pre=files.filter(n=>!n.endsWith('.client.json')).concat(Array.from({length:189},(_,i)=>i+'.client.json'));assert.equal(nativeReceiptRoom(pre,240,50),189);assert.throws(()=>nativeReceiptRoom([...pre,'next.client.json'],240,50),/native client receipt reserve/);assert.equal(nativeReceiptRoom(Array(239).fill('c.client.json'),240,0),239);assert.throws(()=>nativeReceiptRoom(Array(240).fill('c.client.json'),240,0));assert.throws(()=>nativeReceiptRoom([],240,-1));
});
test('prepared party starts are nearby, separated and avoid actual hazards without actor relocation',()=>{
  const map=collisionMap({width:21,height:21,blocked:[[10,7]],damaging_ground:[[7,10]]}),origin={x:10.5,y:10.5},positions=partyPositions(map,origin,12);assert.equal(positions.length,12);
  for(const p of positions){assert(Math.hypot(p.x-origin.x,p.y-origin.y)>=3&&Math.hypot(p.x-origin.x,p.y-origin.y)<=8);assert(!map.blocked[Math.floor(p.y)*map.width+Math.floor(p.x)]);assert(positions.every(q=>q===p||Math.hypot(p.x-q.x,p.y-q.y)>=2));}
  assert.throws(()=>partyPositions(collisionMap({width:21,height:21,blocked:[[10,9],[9,10],[11,10],[10,11]],damaging_ground:[]}),origin,12),/Insufficient safe/);assert.throws(()=>partyPositions({width:1,height:1,blocked:new Uint8Array(1)},origin,12),/origin must be an actual safe/);
});
test('original cohort identity rejects a mismatched class or controller',()=>{
  const spec={class:0x310,level:20,stats:[670,385,50,25,50,50,40,75],equipment:[0xaf6,0xb25,0xb05,0xac5],backpack:Array(8).fill(-1)},v={actor:1,controller:'native',lifecycle:{alive:true,death_pending:false,quarantined:false},session:{account:2,character:3},character:{class:0x310,level:20,hp:670,mp:385,base_stats:[...spec.stats],items:[...spec.equipment,...spec.backpack],inventory_revision:4}};assert.equal(originalIdentity('fighter',spec,v).character,3);assert.throws(()=>originalIdentity('fighter',spec,{...v,controller:'game_bot'}));assert.throws(()=>originalIdentity('fighter',spec,{...v,character:{...v.character,class:0x30e}}));
});
test('replacement binds its exact reserved entrance even when the pool creates another spare',()=>{
  const portals=[{id:10,kind:0x712,published:true},{id:11,kind:0x712,published:true}],reservation={message:'Replacement Realm entrance position reserved',realm:9,portal:11};assert.equal(replacementEntrance(9,portals,[reservation]).id,11);
  assert.throws(()=>replacementEntrance(8,portals,[reservation]),/one exact/);assert.throws(()=>replacementEntrance(9,portals,[reservation,reservation]),/one exact/);assert.throws(()=>replacementEntrance(9,[{...portals[1],published:false}],[reservation]),/not published/);
});
test('the other entrance remains exact or has its own dependency-free surplus retirement, never unexplained absence',()=>{
  const entrance={id:10,realm:1,position:{x:2.5,y:3.5}},portal={id:10,kind:0x712,pos:entrance.position,published:true},retired={message:'Realm population decision',realm:1,reason:'surplus',decision:'300s empty dependency-free surplus retired',dependencies:0,empty_spares:2};assert.equal(otherEntrance(entrance,[portal],[]).disposition,'present');assert.equal(otherEntrance(entrance,[],[retired]).disposition,'source_logged_idle_retirement');
  for(const events of [[],[{...retired,realm:9}],[{...retired,dependencies:1}],[{...retired,empty_spares:1}],[retired,retired]])assert.throws(()=>otherEntrance(entrance,[],events),/lawful idle/);assert.throws(()=>otherEntrance(entrance,[{...portal,published:false}],[]),/unexpectedly closed/);assert.throws(()=>otherEntrance(entrance,[{...portal,pos:{x:3.5,y:3.5}}],[]));
});
test('measured depleted combat reserve escapes before ordinary regeneration is considered complete',()=>{
  const depleted={hp:442,mp:69,stats:{max_hp:790,max_mp:440}};assert.deepEqual(recoveryState(depleted),{retreat:true,recovered:false});
  assert.equal(recoveryState({...depleted,hp:790,mp:110}).retreat,false);assert.equal(recoveryState({...depleted,hp:790,mp:395}).recovered,false);assert.equal(recoveryState({...depleted,hp:790,mp:396}).recovered,true);
  assert.equal(recoveryState({...depleted,hp:450,mp:440}).retreat,true);assert.throws(()=>recoveryState({...depleted,hp:0}),/living measured/);assert.throws(()=>recoveryState({...depleted,mp:NaN}),/living measured/);
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
