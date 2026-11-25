import assert from 'node:assert/strict';
import {readFile,readdir,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {connect} from '../../scripts/record-demo.mjs';
const out=process.env.QA_RESULTS_DIR;
// Seed 25 keeps the ordinary director roll but selects the Lord on the first qualifying defeat.
export const realmSeed=25;
// Explicitly budget native receipts for both contributors, all four quarters and pickup.
export const nativeReceiptLimit=448;
// Keep this Lord route separate from the Ent Soulbound packaging follow-up.
const questKinds=[0x675,0x678,0x668,0x67d,0x91b,0x928];
const spec={class:0x307,level:20,seed:817,equipment:[0xc02,0xb28,0xaf9,0xac0],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50]};
const cappedQuestXp=(50+(spec.level-1)*100)/2;
const history=async()=>(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
let sites,refuge,reserved,world;
export async function prepare(api){
  const events=await history(),ends=events.flatMap((f,i)=>f.message==='Realm simulation populated'?[i]:[]),candidates=[];
  assert(ends.length===2,'Two initial ordinary Realms required');
  for(const [index,id] of [-3,1].entries()){
    const info=await api({op:'world_info',world:id}),batch=events.slice(index===0?0:ends[index-1]+1,ends[index]);
    assert(info.realm_layout===events[ends[index]].layout,'World and population log must agree');
    candidates.push({world:id,info,sites:batch.filter(f=>f.message==='linked Realm site placed'&&questKinds.includes(f.kind)),reserved:batch.filter(f=>f.message==='Realm event reserved outside active indexes'&&f.kind===0xd50)});
  }
  candidates.sort((a,b)=>b.world-a.world);
  ({sites,reserved,world}=candidates[0]);
  const [x,y]=candidates[0].info.spawn_points[0];refuge={x,y};
  assert(reserved.length===1,'One Lord reserve per ordinary Realm required');
  assert(sites.length,'Ordinary quest sites required for event admission');
  sites.sort((a,b)=>questKinds.indexOf(a.kind)-questKinds.indexOf(b.kind));
  await writeFile(join(out,'sites.json'),JSON.stringify({candidates,world,sites}));
  return {...spec,world,position:refuge};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const records=[],windows=[];let until=0,modifiedUntil=0,scoutUntil=0,scoutModifier=0,lastTeleport=-10000,companion,companionUntil=0,peerModifier=0,cooperate=false;
  const browser=config.native.find(n=>n.actor===game.actor),connection=await connect({...browser,url:config.origin+'/',helper:config.browserTools});
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command})});
    const receipt=await response.json();assert(response.ok&&(command.op==='inspect'?receipt.diagnostic!==undefined:receipt.status==='succeeded'),JSON.stringify(receipt));return receipt;
  }
  const scout=(await api({op:'spawn',spec:{...spec,world,guid:'lost-lands-scout',position:refuge,controller:'direct',profile:null}})).result.actor;
  const scoutName=(await api({op:'inspect',actor:scout})).diagnostic.character.name;
  async function prepareScout(change){
    for(let n=0;n<100;n++){
      const life=(await api({op:'inspect',actor:scout})).view.lifecycle;assert(life.alive&&!life.death_pending&&!life.departing,'Prepared scout must remain alive');
      if(!life.busy)return api({op:'cheat',actor:scout,change});await new Promise(ok=>setTimeout(ok,50));
    }
    throw Error('Scout persistence did not finish before its next prepared mutation');
  }
  async function playing(){
    const deadline=Date.now()+15000;
    while(Date.now()<deadline){if(!(await game.observe()).paused)return;await new Promise(ok=>setTimeout(ok,25));}
    throw Error('Native action did not establish its active bounded window within 15 seconds');
  }
  try{
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  const local=(s,id,range)=>s.view.objects.some(e=>e.id===id&&e.hp>0&&Math.hypot(e.position.x-s.view.position.x,e.position.y-s.view.position.y)<range);
  const phase=(s,id)=>s.diagnostic.xml[id]?.states[1]?.name;
  const inState=(s,id,name)=>s.diagnostic.xml[id]?.states.some(x=>x.name===name);
  async function save(label){const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounter.json'),JSON.stringify(records));return state;}
  async function cheat(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function protect(){
    const now=(await game.observe()).game_ms;
    if(windows.length)assert(now+5000<until,`Prepared segment ${windows.at(-1).label} exhausted its finite protection window`);
    if(now>=until){await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});until=now+60000;}
    if(now>=scoutUntil){await prepareScout({kind:'protection',condition:'invulnerable',duration_ms:60000});scoutUntil=now+60000;}
    if(companion&&now>=companionUntil){await api({op:'cheat',actor:companion.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});companionUntil=now+60000;}
  }
  async function idle(ms){while(ms>0){await protect();const now=(await game.observe()).game_ms,step=Math.min(ms,10000,until-now,scoutUntil-now,...(companion?[companionUntil-now]:[]));await game.act({keys:[],duration_ms:step});ms-=step;}}
  async function recordWindows(){await writeFile(join(out,'combat-window.json'),JSON.stringify({reserve_ms:5000,qualification:'Separate prepared segments on the same encounter; no continuous protection or unprotected survival claim.',windows}));}
  function endSegment(view){
    const segment=windows.at(-1);assert(view.lifecycle.alive&&(view.character.conditions&(1<<24))&&view.game_ms+5000<segment.expires_ms,`Prepared segment ${segment.label} did not finish alive with its 5s reserve`);
    segment.ended_ms=view.game_ms;
  }
  async function beginSegment(label){
    if(windows.length)endSegment(await game.observe());
    // Only idle at the boundary; use the final short act to minimize time without protection.
    async function restore(player){
      const {character:c}=await player.observe();
      if(c.hp<c.stats.max_hp)await api({op:'cheat',actor:player.actor,change:{kind:'vitals',hp:c.stats.max_hp,mp:c.stats.max_mp}});
    }
    for(let v;(v=await game.observe()).game_ms<until;){
      const now=v.game_ms,left=Math.min(until-now,...(companion&&companionUntil>now?[companionUntil-now]:[]));
      if(left<=1200){await restore(game);if(companion)await restore(companion);}
      await game.act({keys:[],duration_ms:left<=1200?left:Math.min(10000,left-1000)});
      const after=await game.observe();
      if(companion&&after.game_ms>=companionUntil){await api({op:'cheat',actor:companion.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});companionUntil=after.game_ms+60000;}
    }
    const {game_ms:started_ms}=await game.observe();
    await restore(game);
    await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});until=started_ms+60000;
    windows.push({label,started_ms,expires_ms:until});await recordWindows();
  }
  async function at(point,name=scoutName){
    const now=(await game.observe()).game_ms;
    if(now-lastTeleport<10000)await idle(10000-(now-lastTeleport));
    await protect();
    if(name===scoutName)await prepareScout({kind:'teleport',position:point});
    const action=game.act({keys:[],duration_ms:2000});
    const typing=(async()=>{
      await playing();
      async function enter(){try{await connection.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}finally{await connection.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}}
      await enter();await connection.call('Input.insertText',{text:'/teleport '+name});await enter();
    })();
    for(const r of await Promise.allSettled([action,typing]))if(r.status==='rejected')throw r.reason;
    const arrived=await game.observe();assert(Math.hypot(arrived.position.x-point.x,arrived.position.y-point.y)<.2,'Native teleport must reach '+name);lastTeleport=arrived.game_ms;
    if(name===scoutName)await prepareScout({kind:'teleport',position:refuge});
  }
  // Ordinary scout defeats preserve the director roll and reserve native receipts for the encounter.
  async function sourceFire(target){
    const files=await readdir(join(config.state,'testing'));
    assert(files.filter(f=>f.endsWith('.client.json')).length<80&&files.filter(f=>f.endsWith('.receipt.json')).length<600,'No natural Lord admission before the reserved combat command budget');
    await protect();let now=(await game.observe()).game_ms;
    while(Math.min(until,scoutUntil)-now<10000){await prepareScout({kind:'teleport',position:refuge});await idle(Math.min(until,scoutUntil)-now);await protect();now=(await game.observe()).game_ms;}
    assert(target.hp>0,'Ordinary admission target must be alive');
    await prepareScout({kind:'teleport',position:target.pos});
    if(now>=scoutModifier){await prepareScout({kind:'stat_modifier',index:2,amount:100,duration_ms:60000});scoutModifier=now+60000;}
    const action=game.act({keys:[],duration_ms:10000});
    const firing=(async()=>{
      await playing();
      for(let n=0;n<4;n++){
        const state=await api({op:'inspect',actor:scout}),enemy=entity(state,target.id);if(!enemy)break;
        if((await game.observe()).game_ms>now+6500)break;
        await api({op:'act',actor:scout,action:{command:'shoot',angle:Math.atan2(enemy.pos.y-state.view.position.y,enemy.pos.x-state.view.position.x)},duration_ms:1500,pauseAfter:false});
      }
    })();
    for(const r of await Promise.allSettled([action,firing]))if(r.status==='rejected')throw r.reason;
  }
  async function aimState(id){
    const s=await game.inspect(),target=entity(s,id),shots=s.diagnostic.own_projectiles;
    // Let already-fired bow shots resolve before rebinding a crystal they can finish.
    const ceiling=500*(.5+(spec.stats[2]+100)/50)*1.5;
    if(target?.kind===0xd51&&shots.length&&target.hp<=shots.length*ceiling){
      const wait=Math.max(...shots.map(p=>p.born+553))-s.view.time_ms;assert(wait>0&&wait<=553,'Pending bow lifetime must fit the selected 503ms projectile plus one tick');
      await idle(wait);return game.inspect();
    }
    return s;
  }
  async function approach(point,range=1.5,follow=null,fireOnWalk=true,player=game){
    await protect();const s=follow===null?await player.inspect():await aimState(follow),v=s.view;
    if(follow!==null){const target=entity(s,follow);if(!target){assert((await history()).some(f=>['enemy defeated','timed enemy defeat'].includes(f.message)&&f.entity===follow),'Chased target vanished without defeat');return;}if(target.kind===0xd50&&phase(s,follow)==='Death')return;point=target.pos;}
    if(Math.hypot(point.x-v.position.x,point.y-v.position.y)<range)return;
    const key=(x,y)=>x+','+y,cell=p=>key(Math.floor(p.x),Math.floor(p.y));
    const blocked=new Set(v.objects.filter(e=>e.blocks).map(e=>cell(e.position)));
    const clear=new Set(v.tiles.filter(t=>t.ground_speed>0&&!blocked.has(cell(t))&&Math.abs(t.x-v.position.x)<12&&Math.abs(t.y-v.position.y)<12).map(cell));
    const center=c=>{const [x,y]=c.split(',').map(Number);return {x:x+.5,y:y+.5};};
    const targetDistance=p=>Math.hypot(p.x-point.x,p.y-point.y);
    const start=cell(v.position),queue=[start],parents=new Map([[start,null]]);
    let goal=start;
    for(let i=0;i<queue.length;i++){
      const candidate=queue[i];
      if(candidate===cell(point)||targetDistance(center(candidate))<range){goal=candidate;break;}
      if(targetDistance(center(candidate))<targetDistance(center(goal)))goal=candidate;
      const [x,y]=queue[i].split(',').map(Number);
      for(const [dx,dy] of [[1,0],[-1,0],[0,1],[0,-1]]){const next=key(x+dx,y+dy);if(clear.has(next)&&!parents.has(next)){parents.set(next,queue[i]);queue.push(next);}}
    }
    assert(goal!==start||goal===cell(point),'No reachable approach from the visible tiles to '+JSON.stringify(point));
    let next=goal;while(parents.get(next)!==start&&next!==start)next=parents.get(next);
    const waypoint=next===cell(point)?point:center(next);
    const dx=waypoint.x-v.position.x,dy=waypoint.y-v.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
    const distance=Math.hypot(dx,dy),speed=(v.character.conditions&8)?4:(4+5.6*v.character.stats[4]/75)*((v.character.conditions&16384)?1.5:1);
    assert(Number.isFinite(speed)&&speed>0,'Observed movement speed must be finite and positive');
    await player.act({keys,duration_ms:Math.max(50,Math.min(350,Math.round(distance/speed*1000))),...(follow!==null&&fireOnWalk&&local(s,follow,5)?{target:{entity:follow,camera_angle:-Math.PI/4,fire:true}}:{})});
  }
  async function walk(player,point,range){
    for(let n=0;n<30;n++){
      await approach(point,range,null,false,player);
      const {position}=await player.observe();
      if(Math.hypot(point.x-position.x,point.y-position.y)<range)return;
    }
    throw Error('Native walk did not reach reward '+JSON.stringify({point,range,position:(await player.observe()).position}));
  }
  async function fire(id,ms=500,range=5){
    await protect();const state=await aimState(id),target=entity(state,id);
    if(!target){assert((await history()).some(f=>f.message==='enemy defeated'&&f.entity===id),'Target vanished without combat defeat');return;}
    // Selected Doom Bow has 7.04 tile range (data/objects.xml:3123–3141); do not chase into blocked melee.
    if(phase(state,id)==='Death'){await save('dying');await idle(4000);return;}
    if(target.hp<=0){await idle(250);return;}
    if(!local(state,id,range)){
      await approach(target.pos,range-.5,id);return;
    }
    const now=(await game.observe()).game_ms;if(now>=modifiedUntil){await cheat({kind:'stat_modifier',index:2,amount:100,duration_ms:60000});modifiedUntil=now+60000;}
    const input={keys:target.kind===0xd51&&ms>=1500?[' ']:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}};
    const peer=cooperate?await companion.inspect():null;
    if(peer&&local(peer,id,5)){
      if(now>=peerModifier){await api({op:'cheat',actor:companion.actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}});peerModifier=now+60000;}
      await game.act([[{actor:game.actor,input},{actor:companion.actor,input}]]);
    }else await game.act(input);
  }
  const initial=await save('before-admission');
  game.check('startup reserves one Lord outside active indexes',reserved.length===1);
  let admitted;
  for(let siteIndex=0;siteIndex<sites.length;siteIndex++){
    const site=sites[siteIndex];
    await protect();await prepareScout({kind:'teleport',position:{x:site.x,y:site.y}});
    for(let n=0;n<30;n++){
      const state=await api({op:'inspect',actor:scout}),root=entity(state,site.entity),family=state.diagnostic.entities.filter(e=>e.quest===site.entity);
      const target=family.find(e=>[0x91d,0x929].includes(e.kind))??root??family.find(e=>[0x6c2,0x6c1,0x920,0x91c,0x92d].includes(e.kind));if(!target)break;
      await sourceFire(target);
      admitted=(await history()).find(f=>f.message==='quest defeat summoned a Realm event'&&f.kind===0xd50);if(admitted)break;
    }
    await prepareScout({kind:'teleport',position:refuge});await save('quest-'+site.entity);
    const events=(await history()).filter(f=>f.message==='quest defeat summoned a Realm event');
    admitted=events.find(f=>f.kind===0xd50);
    if(admitted)break;
    for(const event of events.filter(f=>[0xd54,0xd56,0xd59].includes(f.kind)))if(!sites.some(s=>s.entity===event.entity))sites.splice(siteIndex+1,0,event);
  }
  assert(admitted,'Bounded ordinary quest defeats did not admit a Lord; do not substitute a spawned fixture');
  await writeFile(join(out,'admission.json'),JSON.stringify({...admitted,preparation_actor:scout,attempted:records.filter(r=>r.label.startsWith('quest-')).map(r=>r.label)}));
  assert(Math.hypot((await game.observe()).position.x-refuge.x,(await game.observe()).position.y-refuge.y)<.2,'Prepared refuge required before protection boundary');
  await beginSegment('first quarter, contact and shield');
  await at({x:admitted.x,y:admitted.y});
  await game.act({keys:[],duration_ms:500},{name:'lost-lands-waiting',when:()=>true});
  const entry=await save('entry'),lord=entity(entry,admitted.entity);assert(lord?.hp===70000&&phase(entry,lord.id)==='Q1');
  game.check('ordinary prepared scout defeats admit the untouched reserved Lord',lord.id===reserved[0].entity&&lord.kind===0xd50&&lord.quest===lord.id);
  for(let n=0;n<8&&(entity(await game.inspect(),lord.id).damage[initial.view.session.character]??0)<=700;n++)await fire(lord.id,500);
  await idle(4000);
  let state=await save('activated');
  // The quarter tosses Guardians, which make Knights; only the Lord carries quest credit (actors.xml:7878–7879,8054).
  const owned=(s,parent,kind)=>s.diagnostic.entities.filter(e=>e.parent===parent&&e.kind===kind);
  const crystals=s=>owned(s,lord.id,0xd51),guardians=owned(state,lord.id,0xd53);
  game.check('native fire damages the Lord during its quarter and the Lord’s Guardians summon owned Knights',
    entity(state,lord.id).hp<69300&&(entity(state,lord.id).damage[initial.view.session.character]??0)>700&&
    guardians.some(guardian=>owned(state,guardian.id,0xd52).length>0));
  const samples=[];let pending,error;
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>samples.push(s)).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
  const attackEnds=(await game.observe()).game_ms+23000;
  try{
    while((await game.observe()).game_ms<attackEnds){
      const attack=await game.inspect(),current=entity(attack,lord.id);
      if(phase(attack,lord.id)==='PowerDown')break;
      // Contact during Vortex avoids making status coverage depend on a lucky firing angle.
      if(inState(attack,lord.id,'Attack2')&&!(attack.view.character.conditions&32768)&&Math.hypot(current.pos.x-attack.view.position.x,current.pos.y-attack.view.position.y)>1){
        await approach(current.pos,1,lord.id,false);
      }else{
        const remaining=attackEnds-(await game.observe()).game_ms;
        if(remaining>0)await idle(Math.min(1000,remaining));
      }
    }
    // The first shield requires 17,500 effective damage (actors.xml Q1), not an idle timeline.
    for(let n=0;n<36&&phase(await game.inspect(),lord.id)==='Q1';n++){
      if((await game.observe()).game_ms+9000>=until)await beginSegment('first quarter: damage threshold');
      await fire(lord.id,1000,3);
    }
    // The executor reserves the toss for a 1500ms flight (src/world/xml/spawn.rs).
    for(let n=0;n<4&&phase(await game.inspect(),lord.id)==='SummonIce';n++)await idle(1000);
  }finally{clearInterval(timer);if(pending)await pending;await writeFile(join(out,'attacks.json'),JSON.stringify(samples));}if(error)throw error;
  state=await save('protected');
  game.check('Lord uses White Beam and Bleeding Vortex projectiles',[83,35].every(speed=>samples.some(s=>s.view.projectiles.some(p=>p.owner===lord.id&&p.speed===speed))));
  game.check('Lord moves during its attack',samples.some(s=>{const e=entity(s,lord.id);return e&&Math.hypot(e.pos.x-lord.pos.x,e.pos.y-lord.pos.y)>.2;}));
  game.check('native encounter applies Bleeding and Slowed despite prepared Invulnerable',[32768,8].every(mask=>samples.some(s=>(s.view.character.conditions&mask)!==0)));
  // PowerDown pins the Lord at its toss centre and is directly observed after the final act.
  assert.equal(phase(state,lord.id),'PowerDown','Native protection toss must be observed');
  const tossState=state;
  const objects=await readFile(new URL('../../data/objects.xml',import.meta.url),'utf8');
  const grounds=await readFile(new URL('../../data/grounds.xml',import.meta.url),'utf8');
  const definitions=(text,tag)=>[...text.matchAll(new RegExp(`<${tag} type="([^"]+)"[^>]*>([\\s\\S]*?)</${tag}>`,'g'))];
  const blockingKinds=new Set(definitions(objects,'Object').filter(([, ,body])=>/<(?:EnemyOccupySquare|FullOccupy)\s*\/>/.test(body)).map(([,kind])=>Number(kind)));
  const voidGrounds=new Set(definitions(grounds,'Ground').filter(([, ,body])=>/<CompositePriority>-/.test(body)).map(([,kind])=>Number(kind)));
  const center=entity(tossState,lord.id).pos,landings=Array.from({length:8},(_,n)=>{
    const position={x:center.x+4*Math.cos(n*Math.PI/4),y:center.y+4*Math.sin(n*Math.PI/4)},x=Math.floor(position.x),y=Math.floor(position.y);
    const tile=tossState.view.tiles.find(t=>t.x===x&&t.y===y);assert(tile,'Every requested crystal landing must be visible');
    const blockers=tossState.view.objects.filter(e=>blockingKinds.has(e.kind)&&Math.floor(e.position.x)===x&&Math.floor(e.position.y)===y);
    return {position,tile,blockers,placeable:!voidGrounds.has(tile.ground)&&!blockers.length};
  });
  await writeFile(join(out,'protection-landings.json'),JSON.stringify(landings));
  const expected=landings.filter(p=>p.placeable),born=crystals(state);
  game.check('all NPC-placeable radial crystal landings spawn owned protection',expected.length>0&&born.length===expected.length&&
    expected.every(({position})=>born.some(crystal=>Math.hypot(crystal.home.x-position.x,crystal.home.y-position.y)<.15))&&
    phase(state,lord.id)==='PowerDown'&&!!(entity(state,lord.id).condition&(1<<24)));
  const hp=entity(state,lord.id).hp,protectionIds=crystals(state).map(e=>e.id);await fire(lord.id,1000);state=await save('shield-rejects');
  game.check('shield rejects native bow damage',entity(state,lord.id).hp===hp);
  const nearestCrystal=s=>crystals(s).sort((a,b)=>Math.hypot(a.pos.x-s.view.position.x,a.pos.y-s.view.position.y)-Math.hypot(b.pos.x-s.view.position.x,b.pos.y-s.view.position.y))[0];
  await beginSegment('solo first shield: opening crystals');
  let secondBatch=false;
  // The observed solo route can take more than one epoch for all eight orbiting crystals.
  for(let n=0;n<120;n++){
    const s=await game.inspect(),target=nearestCrystal(s);
    if(!target)break;
    if(!secondBatch&&crystals(s).length<=Math.ceil(protectionIds.length/2)){
      await beginSegment('solo first shield: remaining crystals');secondBatch=true;continue;
    }
    await fire(target.id,1600);
  }
  await idle(500);state=await save('shield-released');
  const defeats=await history();
  game.check('native crystal defeats release the shield without waiting for decay',protectionIds.every(id=>defeats.some(f=>f.message==='enemy defeated'&&f.entity===id))&&!crystals(state).length&&phase(state,lord.id)!=='PowerDown'&&!(entity(state,lord.id).condition&(1<<24)));
  await beginSegment('second quarter and native cofighter');
  companion=await game.join({...spec,world,seed:818,position:entity(state,lord.id).pos});await protect();
  const peerBefore=await companion.observe(),peerCharacter=peerBefore.session.character;
  const eligibility=[];
  for(let n=0;n<72;n++){
    const s=await companion.inspect(),boss=entity(s,lord.id);
    assert(boss,'The cofighter lost sight of the live Lord');
    const now=(await companion.observe()).game_ms,damage=boss.damage[peerCharacter]??0,stage=phase(s,lord.id);
    eligibility.push({now,damage,stage,hp:boss.hp,boss:boss.pos,peer:s.view.position});
    if(damage>=7000)break;
    if(now+9000>=until){await beginSegment('second quarter: cofighter eligibility');continue;}
    assert.equal(stage,'Q2','Cofighter must earn eligibility in the second quarter, before the next shield');
    if(now>=peerModifier){await api({op:'cheat',actor:companion.actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}});peerModifier=now+60000;}
    // The Lord can walk out of a long hold. Stay close enough for the slow-rate Doom Bow to hit.
    if(!local(s,lord.id,2.75))await approach(boss.pos,2.5,null,false,companion);
    else await companion.act({keys:[' '],duration_ms:650,target:{entity:lord.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const cofighter=await companion.inspect(),contributions=entity(cofighter,lord.id)?.damage;
  await writeFile(join(out,'eligibility.json'),JSON.stringify({eligibility,contributions}));
  // The guaranteed tier-2 potion row requires 10% of 70,000 HP (actors.xml).
  game.check('two native players each meet the Lord potion eligibility threshold',contributions&&[peerCharacter,initial.view.session.character].every(id=>contributions[id]>=7000));
  // Q2 can move the fight beyond the primary's locally discovered tiles while the peer earns damage.
  if((await game.observe()).game_ms+12000>=until)await beginSegment('second quarter: regroup');
  await at((await companion.observe()).position,cofighter.diagnostic.character.name);
  state=await save('cooperative');
  assert(entity(state,lord.id),'Primary archer must rejoin the native cofighter at the Lord');
  cooperate=true;
  const beforeFinalXp=state.view.character.experience,existingBags=new Set([...state.diagnostic.entities,...(await companion.inspect()).diagnostic.entities].filter(e=>e.items).map(e=>e.id));
  const laterShields=new Map();
  for(let n=0;n<240&&(state=await game.inspect(),entity(state,lord.id));n++){
    const boss=entity(state,lord.id),stage=phase(state,lord.id);
    if(stage==='Death'){await save('dying');await idle(4000);continue;}
    if(stage==='SummonIce'){await idle(1600);continue;}
    const crystal=nearestCrystal(state);
    if(crystal&&stage==='PowerDown'){
      const wave=boss.hp<17500?'Final':'Third',count=crystals(state).length,batch=laterShields.get(wave);
      if(!batch||(!batch.remaining&&count<=Math.ceil(batch.opening/2))){
        laterShields.set(wave,batch?{...batch,remaining:true}:{opening:count,remaining:false});
        await beginSegment(`${wave} quarter: ${batch?'remaining':'opening'} crystals`);continue;
      }
    }
    if(!crystal&&stage==='Q4'){
      const label=windows.at(-1).label;
      // Consecutive renewals let the untracked Lord wander out of both clients' neighborhoods.
      if(label!=='final quarter: timed defeat and pickup'){
        await beginSegment('final quarter: timed defeat and pickup');continue;
      }
      const peer=await companion.observe();
      if(Math.hypot(peer.position.x-boss.pos.x,peer.position.y-boss.pos.y)>7){await approach(boss.pos,6.5,null,false,companion);continue;}
    }
    await fire(crystal?.id??lord.id,1500);
  }
  await idle(1000);state=await save('defeated');
  const death=records.find(r=>r.label==='dying')?.state;
  const publications=(await history()).filter(f=>f.message==='XML terminal effects published from exact durable journal receipt'&&f.actor===lord.id&&f.kind===lord.kind);
  game.check('native combat completes timed Lord defeat and earns XP',death&&phase(death,lord.id)==='Death'&&!entity(state,lord.id)&&state.view.character.experience>=beforeFinalXp+cappedQuestXp&&publications.length===1&&publications[0].xp&&publications[0].loot);
  const event=publications[0].event;
  const journal=()=>JSON.parse(execFileSync('python3',['-c',`import json,sqlite3,sys\nfrom pathlib import Path\nc=sqlite3.connect(Path(sys.argv[1]).as_uri()+'?mode=ro',uri=True)\npayload,=c.execute('SELECT payload FROM world_events WHERE event=?',(sys.argv[2],)).fetchone()\nprint(json.dumps([json.loads(payload)['payload']['data']['actions']['publication'],c.execute('SELECT owner_character,item,status FROM world_entitlements WHERE event=?',(sys.argv[2],)).fetchall()]))`,join(config.state,'game.db'),event],{encoding:'utf8'}));
  const [publication,entitlements]=journal();
  assert.equal(publication.reason,'Explode','The Lord must finish through its timed XML lifecycle');
  const peerAfter=await companion.inspect();
  game.check('both native contributors receive final Lord XP',peerAfter.view.character.experience>=peerBefore.character.experience+cappedQuestXp);
  await writeFile(join(out,'multiplayer.json'),JSON.stringify({peerBefore,peerAfter,damage:records.find(r=>r.label==='activated').state}));
  const xp=state.view.character.experience;await idle(2000);
  game.check('defeat XP is once only',(await game.observe()).character.experience===xp);
  game.check('same living prepared character survives',state.view.lifecycle.alive&&state.view.session.character===initial.view.session.character);
  await game.act({keys:[],duration_ms:500},{name:'lost-lands-defeated',when:()=>true});
  // The potion minimum applies across eligible recipients, not once per contributor.
  const participants=[{player:game,state:await save('rewards')},{player:companion,state:await companion.inspect()}];
  const rewards=participants.flatMap(({player,state})=>state.diagnostic.entities.filter(e=>!existingBags.has(e.id)&&e.items?.some(i=>i!==-1)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===state.view.session.account)&&entitlements.some(([character,item])=>character===state.view.session.character&&e.items.includes(item))).map(bag=>({player,bag,state})));
  assert(rewards.length,'No natural Lord private item observed; pickup/reconnect not exercised');
  const {player:recipient,bag,state:ownerBefore}=rewards[0];
  await api({op:'cheat',actor:scout,change:{kind:'teleport',position:bag.pos}});
  const observer=await api({op:'inspect',actor:scout}),other=participants.find(p=>p.player!==recipient).state;
  game.check('nearby noncontributor cannot see the Lord private reward',!observer.diagnostic.entities.some(e=>e.id===bag.id));
  game.check('nearby native contributor cannot see another account private reward',Math.hypot(other.view.position.x-bag.pos.x,other.view.position.y-bag.pos.y)<12&&!other.diagnostic.entities.some(e=>e.id===bag.id));
  await api({op:'cheat',actor:scout,change:{kind:'teleport',position:refuge}});
  await walk(recipient,bag.pos,.5);
  await recipient.act({keys:[],duration_ms:700},{name:'lost-lands-reward',when:()=>true});
  const inventory=(await recipient.observe()).character,slot=bag.items.findIndex(i=>entitlements.some(([character,item])=>character===ownerBefore.view.session.character&&item===i)),item=bag.items[slot],from={x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44};
  assert(inventory.items[4]===-1,'Empty prepared destination required');
  await recipient.act({keys:[],duration_ms:350,drag:{from,to:{x:634,y:428}}});
  const picked=await recipient.inspect();
  const remaining=entity(picked,bag.id);
  const claimed=journal()[1].filter(([character,value,status])=>character===picked.view.session.character&&value===item&&status===1);
  game.check('native drag commits one eligible Lord item',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===inventory.inventory_revision+1&&(!remaining||remaining.items[slot]===-1)&&claimed.length===1);
  await recipient.act({keys:[],duration_ms:350,drag:{from,to:{x:678,y:428}}});
  game.check('empty-slot drag cannot duplicate the Lord reward',(await recipient.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  const realmEnd=await game.observe();
  endSegment(realmEnd);await recordWindows();
  game.check('all labeled prepared segments and pickup finish within their finite epochs, without replacing either character',realmEnd.world==='Realm'&&['world','player','character','account'].every(k=>realmEnd.session[k]===initial.view.session[k]&&ownerBefore.view.session[k]===picked.view.session[k])&&windows.every(w=>w.ended_ms>=w.started_ms&&w.ended_ms<w.expires_ms));
  await recipient.act({keys:['Insert'],duration_ms:250});
  const retained=await recipient.observe();
  game.check('Nexus reconnect preserves the living character and Lord item',retained.world==='Nexus'&&retained.lifecycle.alive&&retained.session.character===picked.view.session.character&&JSON.stringify(retained.character.items)===JSON.stringify(picked.view.character.items)&&retained.character.inventory_revision===picked.view.character.inventory_revision);
  await writeFile(join(out,'reward.json'),JSON.stringify({event,publication,entitlements,bag,observer,picked,realmEnd,retained,windows}));
  }finally{connection.close();}
}
