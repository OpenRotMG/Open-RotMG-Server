import assert from 'node:assert/strict';
import {readFile,readdir,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';
const out=process.env.QA_RESULTS_DIR;
// Keep this Lord route separate from the Ent Soulbound packaging follow-up.
const questKinds=[0x675,0x678,0x668,0x67d,0x91b,0x928];
const spec={class:0x31d,level:20,seed:817,equipment:[0xb0b,0xb29,0xafc,0xac0],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
const cappedQuestXp=(50+(spec.level-1)*100)/2;
const history=async()=>(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
let sites,refuge,reserved,world;
export async function prepare(api){
  const events=await history(),ends=events.flatMap((f,i)=>f.message==='Realm simulation populated'?[i]:[]),candidates=[];
  assert(ends.length===2,'Two initial ordinary Realms required');
  for(const [index,id] of [-3,1].entries()){
    const info=await api({op:'world_info',world:id}),batch=events.slice(index===0?0:ends[index-1]+1,ends[index]);
    assert(info.realm_layout===events[ends[index]].layout,'World and population log must agree');
    candidates.push({world:id,info,sites:batch.filter(f=>f.message==='finite named Realm quest placed'&&questKinds.includes(f.kind)),reserved:batch.filter(f=>f.message==='Realm event reserved outside active indexes'&&f.kind===0xd50)});
  }
  candidates.sort((a,b)=>b.sites.length-a.sites.length);
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
  const records=[];let until=0,modifiedUntil=0,scoutUntil=0,scoutModifier=0,lastTeleport=-10000,companion,companionUntil=0,peerModifier=0,cooperate=false;
  const browser=config.native.find(n=>n.actor===game.actor),connection=await connect({...browser,url:config.origin+'/',helper:config.browserTools});
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command})});
    const receipt=await response.json();assert(response.ok&&(command.op==='inspect'?receipt.diagnostic!==undefined:receipt.status==='succeeded'),JSON.stringify(receipt));return receipt;
  }
  const scout=(await api({op:'spawn',spec:{...spec,class:0x307,equipment:[0xc02,0xb28,0xaf9,0xac0],stats:[700,252,75,25,50,50,40,50],world,guid:'lost-lands-scout',position:refuge,controller:'direct',profile:null}})).result.actor;
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
  async function save(label){const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounter.json'),JSON.stringify(records));return state;}
  async function cheat(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function protect(){
    const now=(await game.observe()).game_ms;
    if(now>=until){await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});until=now+60000;}
    if(now>=scoutUntil){await prepareScout({kind:'protection',condition:'invulnerable',duration_ms:60000});scoutUntil=now+60000;}
    if(companion&&now>=companionUntil){await api({op:'cheat',actor:companion.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});companionUntil=now+60000;}
  }
  async function idle(ms){while(ms>0){await protect();const now=(await game.observe()).game_ms,step=Math.min(ms,10000,until-now,scoutUntil-now,...(companion?[companionUntil-now]:[]));await game.act({keys:[],duration_ms:step});ms-=step;}}
  async function at(point){
    const now=(await game.observe()).game_ms;
    if(now-lastTeleport<10000)await idle(10000-(now-lastTeleport));
    await protect();
    await prepareScout({kind:'teleport',position:point});
    const action=game.act({keys:[],duration_ms:2000});
    const typing=(async()=>{
      await playing();
      async function enter(){try{await connection.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}finally{await connection.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}}
      await enter();await connection.call('Input.insertText',{text:'/teleport '+scoutName});await enter();
    })();
    for(const r of await Promise.allSettled([action,typing]))if(r.status==='rejected')throw r.reason;
    const arrived=await game.observe();assert(Math.hypot(arrived.position.x-point.x,arrived.position.y-point.y)<.2,'Native teleport must reach direct scout');lastTeleport=arrived.game_ms;
    await prepareScout({kind:'teleport',position:refuge});
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
  async function walk(point,range=1.5,follow=null){
    for(let n=0;n<30;n++){
      await protect();const s=await game.inspect(),v=s.view;
      if(follow!==null){const target=entity(s,follow);if(!target){assert((await history()).some(f=>['enemy defeated','timed enemy defeat'].includes(f.message)&&f.entity===follow),'Chased target vanished without defeat');return;}point=target.pos;}
      if(Math.hypot(point.x-v.position.x,point.y-v.position.y)<range)return;
      const key=(x,y)=>x+','+y,cell=p=>key(Math.floor(p.x),Math.floor(p.y));
      const blocked=new Set(v.objects.filter(e=>e.blocks).map(e=>cell(e.position)));
      const clear=new Set(v.tiles.filter(t=>t.ground_speed>0&&!blocked.has(cell(t))&&Math.abs(t.x-v.position.x)<12&&Math.abs(t.y-v.position.y)<12).map(cell));
      const start=cell(v.position),goal=cell(point),queue=[start],parents=new Map([[start,null]]);
      for(let i=0;i<queue.length&&!parents.has(goal);i++){
        const [x,y]=queue[i].split(',').map(Number);
        for(const [dx,dy] of [[1,0],[-1,0],[0,1],[0,-1]]){const next=key(x+dx,y+dy);if(clear.has(next)&&!parents.has(next)){parents.set(next,queue[i]);queue.push(next);}}
      }
      assert(parents.has(goal),'No visible walkable route to observed target '+JSON.stringify(point));
      let next=goal;while(parents.get(next)!==start&&next!==start)next=parents.get(next);
      const [tx,ty]=next.split(',').map(Number),waypoint=next===goal?point:{x:tx+.5,y:ty+.5};
      const dx=waypoint.x-v.position.x,dy=waypoint.y-v.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      const distance=Math.hypot(dx,dy),speed=(4+5.6*v.character.stats[4]/75)*((v.character.conditions&8)?0.5:1);
      assert(Number.isFinite(speed)&&speed>0,'Observed movement speed must be finite and positive');
      await game.act({keys,duration_ms:Math.max(50,Math.min(350,Math.round(distance/speed*1000)))});
    }
    throw Error('Native walk did not reach target '+JSON.stringify({point,range,position:(await game.observe()).position}));
  }
  async function fire(id,ms=500){
    await protect();const state=await game.inspect(),target=entity(state,id);assert(target,'Living target required');
    if(Math.hypot(target.pos.x-state.view.position.x,target.pos.y-state.view.position.y)>2){await walk(target.pos,1.5,id);if(!entity(await game.inspect(),id))return;}
    const current=entity(await game.inspect(),id);if(current?.kind===0xd50&&current.stage>=9){await save('dying');await idle(2500);return;}
    const now=(await game.observe()).game_ms;if(now>=modifiedUntil){await cheat({kind:'stat_modifier',index:2,amount:100,duration_ms:60000});modifiedUntil=now+60000;}
    const input={keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}};
    if(cooperate&&target.kind===0xd50){
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
  await at({x:admitted.x,y:admitted.y});
  await game.act({keys:[],duration_ms:500},{name:'lost-lands-waiting',when:()=>true});
  const entry=await save('entry'),lord=entity(entry,admitted.entity);assert(lord?.hp===70000&&lord.stage===0);
  game.check('ordinary prepared scout defeats admit the untouched reserved Lord',lord.id===reserved[0].entity&&lord.kind===0xd50&&lord.quest===lord.id);
  companion=await game.join({...spec,world,seed:818,position:lord.pos});await protect();
  const peerBefore=await companion.observe(),peerCharacter=peerBefore.session.character;
  for(let n=0;n<8&&(entity(await game.inspect(),lord.id).damage[peerCharacter]??0)<=700;n++)await companion.act({keys:[],duration_ms:500,target:{entity:lord.id,camera_angle:-Math.PI/4,fire:true}});
  for(let n=0;n<8&&(entity(await game.inspect(),lord.id).damage[initial.view.session.character]??0)<=700;n++)await fire(lord.id,500);
  await idle(4000);
  let state=await save('activated');
  game.check('two native players each contribute above the Lord private threshold',[peerCharacter,initial.view.session.character].every(id=>entity(state,lord.id).damage[id]>700));
  const family=s=>s.diagnostic.entities.filter(e=>e.quest===lord.id),crystals=s=>family(s).filter(e=>e.kind===0xd51);
  game.check('native damage activates Lord and summons Guardians and Knights',entity(state,lord.id).hp<69300&&[0xd52,0xd53].every(k=>family(state).some(e=>e.kind===k)));
  const samples=[];let pending,error;
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>samples.push(s)).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
  const attackEnds=(await game.observe()).game_ms+23000;
  try{
    while((await game.observe()).game_ms<attackEnds){
      const current=entity(await game.inspect(),lord.id);
      // Contact during Vortex avoids making status coverage depend on a lucky firing angle.
      if(current.stage===6)await walk(current.pos,.5,lord.id);
      const remaining=attackEnds-(await game.observe()).game_ms;
      if(remaining>0)await idle(Math.min(3000,remaining));
    }
  }finally{clearInterval(timer);if(pending)await pending;}if(error)throw error;
  await writeFile(join(out,'attacks.json'),JSON.stringify(samples));state=await save('protected');
  game.check('Lord uses White Beam and Bleeding Vortex projectiles',[83,35].every(speed=>samples.some(s=>s.view.projectiles.some(p=>p.owner===lord.id&&p.speed===speed))));
  game.check('Lord moves during its attack',samples.some(s=>{const e=entity(s,lord.id);return e&&Math.hypot(e.pos.x-lord.pos.x,e.pos.y-lord.pos.y)>.2;}));
  game.check('native encounter applies Bleeding and Slowed despite prepared Invulnerable',[32768,8].every(mask=>samples.some(s=>(s.view.character.conditions&mask)!==0)));
  const tossState=samples.find(s=>entity(s,lord.id)?.stage===7);assert(tossState,'Native protection toss must be observed');
  const center=entity(tossState,lord.id).pos,landings=Array.from({length:8},(_,n)=>{
    const position={x:center.x+4*Math.cos(n*Math.PI/4),y:center.y+4*Math.sin(n*Math.PI/4)},x=Math.floor(position.x),y=Math.floor(position.y);
    const tile=tossState.view.tiles.find(t=>t.x===x&&t.y===y);assert(tile,'Every requested crystal landing must be visible');
    const blockers=tossState.view.objects.filter(e=>e.blocks&&Math.floor(e.position.x)===x&&Math.floor(e.position.y)===y);
    return {position,tile,blockers,walkable:tile.ground_speed>0&&!blockers.length};
  });
  await writeFile(join(out,'protection-landings.json'),JSON.stringify(landings));
  const expected=landings.filter(p=>p.walkable).length,births=(await history()).filter(f=>f.message==='bounded named quest followers spawned'&&f.quest===lord.id&&f.kind===0xd51).reduce((n,f)=>n+f.count,0);
  game.check('all walkable radial crystal landings spawn owned protection',expected>0&&expected===births&&crystals(state).length===expected&&entity(state,lord.id).stage===8&&!!(entity(state,lord.id).condition&(1<<24)));
  const hp=entity(state,lord.id).hp;await fire(lord.id,1000);state=await save('shield-rejects');
  game.check('shield rejects native sword damage',entity(state,lord.id).hp===hp);
  const protectionIds=crystals(state).map(e=>e.id);
  for(const id of protectionIds)for(let n=0;n<35&&entity(await game.inspect(),id);n++)await fire(id);
  await idle(500);state=await save('shield-released');
  const defeats=await history();
  game.check('native crystal defeats release the shield without waiting for decay',protectionIds.every(id=>defeats.some(f=>f.message==='enemy defeated'&&f.entity===id))&&!crystals(state).length&&entity(state,lord.id).stage!==8&&!(entity(state,lord.id).condition&(1<<24)));
  cooperate=true;
  const beforeFinalXp=state.view.character.experience,existingBags=new Set(state.diagnostic.entities.filter(e=>e.items).map(e=>e.id));
  for(let n=0;n<300&&entity(await game.inspect(),lord.id);n++){
    state=await game.inspect();
    if(entity(state,lord.id).stage>=9){await save('dying');await idle(2500);continue;}
    const crystal=crystals(state)[0];
    await fire(crystal?.id??lord.id,1000);
  }
  await idle(1000);state=await save('defeated');
  game.check('native combat completes timed Lord defeat and earns XP',!entity(state,lord.id)&&state.view.character.experience>=beforeFinalXp+cappedQuestXp&&(await history()).some(f=>f.message==='timed enemy defeat'&&f.entity===lord.id));
  const peerAfter=await companion.inspect();
  game.check('both native contributors receive final Lord XP',peerAfter.view.character.experience>=peerBefore.character.experience+cappedQuestXp);
  await writeFile(join(out,'multiplayer.json'),JSON.stringify({peerBefore,peerAfter,damage:records.find(r=>r.label==='activated').state}));
  const xp=state.view.character.experience;await idle(2000);
  game.check('defeat XP is once only',(await game.observe()).character.experience===xp);
  game.check('same living prepared character survives',state.view.lifecycle.alive&&state.view.session.character===initial.view.session.character);
  await game.act({keys:[],duration_ms:500},{name:'lost-lands-defeated',when:()=>true});
  state=await save('rewards');
  // Verify an observed private roll, not historical rates: https://web.archive.org/web/20120521061126id_/http://forums.wildshadow.com/wiki/realm-mad-god/lord-lost-lands
  const bag=state.diagnostic.entities.find(e=>!existingBags.has(e.id)&&e.items?.some(i=>i!==-1)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===state.view.session.account));
  assert(bag,'No natural Lord private item observed; pickup/reconnect not exercised');
  await api({op:'cheat',actor:scout,change:{kind:'teleport',position:bag.pos}});
  const observer=await api({op:'inspect',actor:scout}),peerReward=await companion.inspect();
  game.check('nearby noncontributor cannot see the Lord private reward',!observer.diagnostic.entities.some(e=>e.id===bag.id));
  game.check('nearby native contributor cannot see another account private reward',Math.hypot(peerReward.view.position.x-bag.pos.x,peerReward.view.position.y-bag.pos.y)<12&&!peerReward.diagnostic.entities.some(e=>e.id===bag.id));
  await api({op:'cheat',actor:scout,change:{kind:'teleport',position:refuge}});
  await walk(bag.pos,.5);
  await game.act({keys:[],duration_ms:700},{name:'lost-lands-reward',when:()=>true});
  const inventory=(await game.observe()).character,slot=bag.items.findIndex(i=>i!==-1),item=bag.items[slot],from={x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44};
  assert(inventory.items[4]===-1,'Empty prepared destination required');
  await game.act({keys:[],duration_ms:350,drag:{from,to:{x:634,y:428}}});
  const picked=await save('picked');
  game.check('native drag commits one eligible Lord item',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===inventory.inventory_revision+1&&entity(picked,bag.id)?.items[slot]===-1);
  await game.act({keys:[],duration_ms:350,drag:{from,to:{x:678,y:428}}});
  game.check('empty-slot drag cannot duplicate the Lord reward',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  await game.act({keys:['Insert'],duration_ms:250});
  const retained=await game.observe();
  game.check('Nexus reconnect preserves the living character and Lord item',retained.world==='Nexus'&&retained.lifecycle.alive&&retained.session.character===picked.view.session.character&&JSON.stringify(retained.character.items)===JSON.stringify(picked.view.character.items)&&retained.character.inventory_revision===picked.view.character.inventory_revision);
  await writeFile(join(out,'reward.json'),JSON.stringify({bag,observer,picked,retained}));
  }finally{connection.close();}
}
