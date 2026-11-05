import assert from 'node:assert/strict';
import {join} from 'node:path';

export function structuredEvents(origin){
  let started=false;
  return line=>{
    if(!line.trim())return null;
    if(line.startsWith('{')){const row=JSON.parse(line);assert(row.fields&&typeof row.fields.message==='string','Invalid structured server event');started=true;return row.fields;}
    // run.py precedes Rust events with build output; only these two ready lines follow them.
    assert(!started||line===`Local server ready. Open: ${origin}/`||line==='Logs: the terminal and .local/server.log','Unexpected unstructured runtime output: '+line);
    return null;
  };
}
export function earnedLedger(realm,initial,priorEvents){
  assert(Array.isArray(priorEvents),'Explicit owned log history required');
  assert(Number.isSafeInteger(realm)&&Array.isArray(initial)&&initial.length>0&&new Set(initial).size===initial.length&&initial.every(id=>Number.isSafeInteger(id)&&id>0),'An unreduced bound Realm requires distinct positive initial roots');
  const known=new Set(initial),pending=new Set(initial),credited=new Set(),admitted=new Set(),waiting=new Set();let census=false,last=null;
  const state={ingest(f){
    if(f.message==='finite Realm closure roster bound'&&f.origin===realm){assert(!census&&f.heroes===initial.length,'Missing, duplicate or mismatched normal census');census=true;}
    if(f.message==='quest defeat summoned a Realm event'&&known.has(f.source)){
      assert(waiting.has(f.source)&&pending.has(f.source)&&Number.isSafeInteger(f.entity)&&f.entity>0&&!known.has(f.entity)&&admitted.size<10,'Unowned, duplicate or invalid earned event');
      known.add(f.entity);admitted.add(f.entity);pending.add(f.entity);waiting.delete(f.source);
    }
    if(f.message==='quest defeat earned a pending Realm event'&&known.has(f.source)){assert(pending.has(f.source)&&!waiting.has(f.source),'Invalid pending event source/order');waiting.add(f.source);}
    if(f.message==='qualifying Realm hero defeated'&&f.origin===realm){
      assert(census&&pending.has(f.hero)&&!waiting.has(f.hero)&&Number.isSafeInteger(f.now)&&f.now>=0&&(last===null||f.now>=last.now),'Credit requires its actual coordinator clock and an uncredited settled root');
      assert.equal(f.remaining,pending.size-1,'Local and coordinator ledgers differ');pending.delete(f.hero);credited.add(f.hero);last=f;
    }
  },snapshot:()=>({realm,census,initial:[...initial],admitted:[...admitted],credited:[...credited],pending:[...pending],waiting:[...waiting],last}),complete(){
    const s=this.snapshot();assert(census&&pending.size===0&&waiting.size===0&&credited.size===initial.length+admitted.size,'All initial and admitted roots must complete');
    assert(admitted.size>0,'No earned event was exercised');return s;
  }};
  for(const event of priorEvents)state.ingest(event);return state;
}
export async function readMap(origin,headers,player,entitySummary,request=fetch){
  assert.equal(typeof entitySummary,'boolean');
  const r=await request(origin+'/dev/map?player='+player+'&entity_summary='+entitySummary,{headers,signal:AbortSignal.timeout(10000)});assert(r.ok);const data=await r.json();
  if(entitySummary)assert.equal(data.projection,'entity_summary');else assert(!Object.hasOwn(data,'projection'));return data;
}
export function mapInput(layout,catalogue){
  const match=layout.match(/^classic-world-(\d+)-reconstruction$/);
  if(match){const recipe='world'+match[1]+'.json',row=catalogue.find(r=>r.kind==='reconstruction'&&r.recipe===recipe);assert(row,'Reconstruction map must be catalogued');return {recipe,sha256:row.sha256};}
  const row=catalogue.find(r=>r.kind==='baseline'&&r.id===layout);assert(row,'Selected layout requires an explicit catalogued baseline');return {file:join('data',row.path),sha256:row.sha256};
}
export function collisionMap(terrain){
  const {width,height}=terrain;assert(Number.isSafeInteger(width)&&width>0&&Number.isSafeInteger(height)&&height>0&&width*height<=16*1024*1024,'Bounded actual map dimensions required');
  assert(Array.isArray(terrain.blocked)&&Array.isArray(terrain.damaging_ground),'Actual runtime solids and hazards required');const blocked=new Uint8Array(width*height);
  for(const [x,y]of [...terrain.blocked,...terrain.damaging_ground]){assert(Number.isInteger(x)&&Number.isInteger(y)&&x>=0&&y>=0&&x<width&&y<height,'Runtime collision coordinate outside map');blocked[y*width+x]=1;}
  return {width,height,blocked};
}
export function route(map,start,goal){
  const {width,height,blocked}=map,total=width*height,parent=new Int32Array(total).fill(-1),queue=new Int32Array(total),origin=Math.floor(start.y)*width+Math.floor(start.x);
  assert(origin>=0&&origin<total);parent[origin]=origin;queue[0]=origin;let head=0,tail=1,target;
  while(head<tail){const id=queue[head++],x=id%width,y=Math.floor(id/width);if(Math.hypot(x+.5-goal.x,y+.5-goal.y)<=5.5){target=id;break;}
    for(const n of [id-width,id-1,id+1,id+width])if(n>=0&&n<total&&Math.abs(n%width-x)<=1&&parent[n]===-1&&!blocked[n]){parent[n]=id;queue[tail++]=n;}}
  assert.notEqual(target,undefined,'No collision-legal undamaging route to the current encounter');
  const path=[];for(let id=target;id!==origin;id=parent[id])path.push(id);path.push(origin);path.reverse();
  return path.filter((p,i)=>i===0||i===path.length-1||p-path[i-1]!==path[i+1]-p).map(id=>({x:id%width+.5,y:Math.floor(id/width)+.5}));
}
export function inputWork(input,actor){
  const steps=Array.isArray(input)?input:[{actor,input}];assert(steps.length>0&&steps.length<=32,'Bounded nonempty native sequence required');
  let duration=0;for(const step of steps){assert(step.actor===actor&&Number.isSafeInteger(step.input?.duration_ms)&&step.input.duration_ms>0&&step.input.duration_ms<=10000,'Each planned input requires its original actor and bounded duration');duration+=step.input.duration_ms;}
  assert(duration<=10000,'Native sequence total exceeds 10000ms');return {duration,inputs:steps.length};
}
export function nativeReceiptRoom(files,limit,reserve){
  assert(Number.isSafeInteger(limit)&&limit>0&&Number.isSafeInteger(reserve)&&reserve>=0);const used=files.filter(n=>n.endsWith('.client.json')).length;
  assert(used+1+reserve<=limit,'Actual native client receipt reserve exhausted');return used;
}
export function partyPositions(map,origin,count){
  assert(Number.isSafeInteger(count)&&count>0&&count<=12);const start={x:Math.floor(origin.x)+.5,y:Math.floor(origin.y)+.5},queue=[start],seen=new Set([start.y*map.width+start.x]);
  assert(start.x>=0&&start.y>=0&&start.x<map.width&&start.y<map.height&&!map.blocked[Math.floor(start.y)*map.width+Math.floor(start.x)],'Party origin must be an actual safe cell');
  for(const p of queue)for(const [dx,dy]of [[0,-1],[-1,0],[1,0],[0,1]]){const q={x:p.x+dx,y:p.y+dy},key=q.y*map.width+q.x;if(q.x>=0&&q.y>=0&&q.x<map.width&&q.y<map.height&&Math.hypot(q.x-origin.x,q.y-origin.y)<=8&&!seen.has(key)&&!map.blocked[Math.floor(q.y)*map.width+Math.floor(q.x)]){seen.add(key);queue.push(q);}}
  const positions=queue.filter(p=>Math.hypot(p.x-origin.x,p.y-origin.y)>=3);
  positions.sort((a,b)=>Math.hypot(a.x-origin.x,a.y-origin.y)-Math.hypot(b.x-origin.x,b.y-origin.y)||a.y-b.y||a.x-b.x);const chosen=[];
  for(const p of positions)if(chosen.every(q=>Math.hypot(p.x-q.x,p.y-q.y)>=2)){chosen.push(p);if(chosen.length===count)return chosen;}
  throw Error('Insufficient safe distinct initial party positions');
}
export function originalIdentity(role,spec,v){
  assert(v.lifecycle.alive&&!v.lifecycle.death_pending&&!v.lifecycle.quarantined&&Number.isSafeInteger(v.character.hp)&&v.character.hp>0&&Number.isSafeInteger(v.character.mp)&&v.character.mp>=0);
  for(const id of [v.actor,v.session.account,v.session.character])assert(Number.isSafeInteger(id)&&id>0);
  assert.equal(v.controller,role==='fighter'||role==='observer'?'native':'game_bot');
  assert.equal(v.character.class,spec.class);assert.equal(v.character.level,spec.level);assert.deepEqual(v.character.base_stats,spec.stats);
  assert.deepEqual(v.character.items,[...spec.equipment,...spec.backpack]);
  return {role,actor:v.actor,account:v.session.account,character:v.session.character,class:spec.class,items:v.character.items,revision:v.character.inventory_revision,initial:v};
}
export function replacementEntrance(realm,entities,events){
  const reserved=events.filter(e=>e.message==='Replacement Realm entrance position reserved'&&e.realm===realm);assert.equal(reserved.length,1,'Replacement requires one exact reserved entrance');
  const portal=entities.find(e=>e.id===reserved[0].portal&&e.kind===0x712&&e.published);assert(portal,'Exact replacement entrance is not published in Nexus');return portal;
}
export function otherEntrance(entrance,entities,events){
  const present=entities.find(e=>e.id===entrance.id);if(present){assert.equal(present.kind,0x712);assert(present.published,'Other entrance unexpectedly closed');assert.deepEqual(present.pos,entrance.position);return {realm:entrance.realm,portal:entrance.id,disposition:'present'};}
  const retired=events.filter(e=>e.message==='Realm population decision'&&e.realm===entrance.realm&&e.reason==='surplus'&&e.decision==='300s empty dependency-free surplus retired'&&e.dependencies===0&&e.empty_spares>=2);assert.equal(retired.length,1,'Other entrance vanished without its exact lawful idle retirement');return {realm:entrance.realm,portal:entrance.id,disposition:'source_logged_idle_retirement',retirement:retired[0]};
}
export function recoveryState(character){
  const {hp,mp,stats:{max_hp,max_mp}}=character;assert([hp,mp,max_hp,max_mp].every(Number.isFinite)&&hp>0&&mp>=0&&max_hp>0&&max_mp>=110,'Recovery requires a living measured character');
  return {retreat:hp<max_hp*.6||mp<110,recovered:hp>=max_hp*.95&&mp>=max_mp*.9};
}
export function assertCastleArrival(view,original){
  assert.equal(view.map.name,"Oryx's Castle");assert.equal(view.session.character,original.session.character);assert.equal(view.session.account,original.session.account);assert(view.lifecycle.alive);
}
export function observedAim(view,entity,angle){
  const target=view.objects.find(e=>e.id===entity&&e.hp>0);assert(target,'Aim requires a living enemy in the observed neighborhood');const dx=target.position.x-view.position.x,dy=target.position.y-view.position.y,x=50*(dx*Math.cos(angle)+dy*Math.sin(angle)),y=50*(-dx*Math.sin(angle)+dy*Math.cos(angle)),scale=Math.max(1,Math.hypot(x,y)/270);assert([x,y,angle].every(Number.isFinite),'Aim geometry must be finite');return {x:300+x/scale,y:325+y/scale};
}
export function localTargets(objects,family,position){
  const candidates=objects.filter(e=>family.some(f=>f.id===e.id)&&e.hp>0&&Math.hypot(e.position.x-position.x,e.position.y-position.y)<=7),mask=(1<<23)|(1<<24)|(1<<21);
  for(const e of candidates)assert(Number.isInteger(e.condition_mask)&&e.condition_mask>=-2147483648&&e.condition_mask<=2147483647,'Combat target requires its observed int32 condition mask');
  const open=candidates.some(e=>(e.condition_mask&mask)===0);return candidates.sort((a,b)=>Number(open&&(a.condition_mask&mask)!==0)-Number(open&&(b.condition_mask&mask)!==0)||a.hp-b.hp);
}
export function textReceiver(){
  const streams=new Map(),texts=[];let error=null;
  return {texts,get error(){return error;},event(e){
    if(e.method!=='Network.webSocketFrameReceived'||e.params.response.opcode!==2)return;
    try{
      const key=e.params.requestId;if(!streams.has(key)){
        const seed=Buffer.from('72c5583cafb6818995cbd74b80','hex'),s=Array.from({length:256},(_,i)=>i);let j=0;
        for(let i=0;i<256;i++){j=(j+s[i]+seed[i%seed.length])&255;[s[i],s[j]]=[s[j],s[i]];}streams.set(key,{s,i:0,j:0,buffer:Buffer.alloc(0)});
      }
      const r=streams.get(key);r.buffer=Buffer.concat([r.buffer,Buffer.from(e.params.response.payloadData,'base64')]);assert(r.buffer.length<=2097152);
      while(r.buffer.length>=5){const n=r.buffer.readInt32BE();assert(n>=5&&n<=1048576);if(r.buffer.length<n)break;
        const id=r.buffer[4],body=Buffer.from(r.buffer.subarray(5,n));r.buffer=r.buffer.subarray(n);
        for(let k=0;k<body.length;k++){r.i=(r.i+1)&255;r.j=(r.j+r.s[r.i])&255;[r.s[r.i],r.s[r.j]]=[r.s[r.j],r.s[r.i]];body[k]^=r.s[(r.s[r.i]+r.s[r.j])&255];}
        if(id===25){let offset=0;const utf=()=>{const size=body.readUInt16BE(offset);offset+=2;assert(offset+size<=body.length);const value=body.toString('utf8',offset,offset+size);offset+=size;return value;};const name=utf();offset+=9;texts.push({name,recipient:utf(),text:utf(),cleanText:utf(),receivedWallMs:Date.now()});assert.equal(offset,body.length);}
      }
    }catch(cause){if(error===null)error=String(cause);}
  }};
}
