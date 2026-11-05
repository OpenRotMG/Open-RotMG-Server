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
export function originalIdentity(role,spec,v){
  assert(v.lifecycle.alive&&!v.lifecycle.death_pending&&!v.lifecycle.quarantined&&Number.isSafeInteger(v.character.hp)&&v.character.hp>0&&Number.isSafeInteger(v.character.mp)&&v.character.mp>=0);
  for(const id of [v.actor,v.session.account,v.session.character])assert(Number.isSafeInteger(id)&&id>0);
  assert.equal(v.controller,role==='fighter'||role==='observer'?'native':'game_bot');
  assert.equal(v.character.class,spec.class);assert.equal(v.character.level,spec.level);assert.deepEqual(v.character.base_stats,spec.stats);
  assert.deepEqual(v.character.items,[...spec.equipment,...spec.backpack]);
  return {role,actor:v.actor,account:v.session.account,character:v.session.character,class:spec.class,items:v.character.items,revision:v.character.inventory_revision,initial:v};
}
export function recoveryState(character){
  const {hp,mp,stats:{max_hp,max_mp}}=character;assert([hp,mp,max_hp,max_mp].every(Number.isFinite)&&hp>0&&mp>=0&&max_hp>0&&max_mp>=110,'Recovery requires a living measured character');
  return {retreat:hp<max_hp*.6||mp<110,recovered:hp>=max_hp*.95&&mp>=max_mp*.9};
}
export function assertCastleArrival(view,original){
  assert.equal(view.map.name,"Oryx's Castle");assert.equal(view.session.character,original.session.character);assert.equal(view.session.account,original.session.account);assert(view.lifecycle.alive);
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
