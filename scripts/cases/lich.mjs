import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {journaledReward} from './reward-journal.mjs';
import {route} from './native-route.mjs';
export const realmSeed=105;
const out=process.env.QA_RESULTS_DIR,branch=process.env.LICH_BRANCH;
const index=['Solo','SmallGroup','Mob','HugeMob'].indexOf(branch);
const spec={class:0x31d,level:20,seed:793,world:1,equipment:[0xb0b,0xb29,0xa7b,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
let root;
export async function prepare(api){
  assert(index>=0,'Set LICH_BRANCH to Solo, SmallGroup, Mob or HugeMob');
  const world=await api({op:'world_info',world:1});
  const events=(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  const end=events.findLastIndex(f=>f.message==='Realm simulation populated'&&f.layout===world.realm_layout);assert(end>=0);
  const start=events.slice(0,end).findLastIndex(f=>f.message==='Realm simulation populated');
  const roots=events.slice(start+1,end).filter(f=>f.message==='linked Realm site placed'&&f.kind===0x91b&&f.layout===world.realm_layout);
  assert(roots.length>=4,'Four ordinarily placed Liches required');
  // The third ruin has a Blue Pillar on the second Mob wave's 240° landing tile.
  root=roots[index===2?3:index];
  await writeFile(join(out,'placement.json'),JSON.stringify({world,roots,branch}));
  return {...spec,...(index>=2?{class:0x307,stats:[700,252,75,25,50,50,40,50],equipment:[0xb02,0xb28,-1,0xacc],backpack:[0xb10,-1,-1,-1,-1,-1,-1,-1]}:{}),position:{x:root.x,y:root.y}};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const records=[],protectedUntil=new Map(),partners=[];
  async function cheat(actor,change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:actor.actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function protect(actor){
    const now=(await actor.observe()).game_ms;
    if(now>=(protectedUntil.get(actor.actor)??0)){await cheat(actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil.set(actor.actor,now+60000);}
  }
  async function idle(ms){
    while(ms>0){const actors=[game,...partners];for(const actor of actors)await protect(actor);const now=(await game.observe()).game_ms,step=Math.min(ms,5000,...actors.map(actor=>protectedUntil.get(actor.actor)-now));await game.act({keys:[],duration_ms:step});ms-=step;}
  }
  const entity=(state,id)=>state.diagnostic.entities.find(e=>e.id===id);
  // Quest forms inherit the quest; their XML ObjectToss/MakeMinions followers are owned by parent (actors.xml:3213,3314).
  const heads=new Set([root.entity]);
  const family=state=>{for(const e of state.diagnostic.entities)if(e.quest===root.entity)heads.add(e.id);return state.diagnostic.entities.filter(e=>heads.has(e.id)||heads.has(e.parent));};
  const phase=state=>(state.diagnostic.xml[root.entity]?.states??[]).map(x=>x.name).at(-1);
  async function save(label){const state=await game.inspect();records.push({branch,label,state});await writeFile(join(out,'encounters.json'),JSON.stringify(records));return state;}
  async function approach(actor,point,range=1.5,aroundPillars=false){
    for(let n=0;n<60;n++){
      await protect(actor);const frame=typeof point==='number'?await actor.inspect():null;
      const goal=frame?entity(frame,point)?.pos:point;assert(goal,'Living approach target required');
      const view=await actor.observe(),p=view.position;
      let waypoint=goal;
      if(aroundPillars){
        const blockers=frame.view.objects.filter(o=>o.blocks).map(o=>o.position);
        const cells=new Set(blockers.map(o=>Math.floor(o.x)+','+Math.floor(o.y)));
        const clear=(a,b)=>Array.from({length:21},(_,i)=>i/20).every(t=>!cells.has(Math.floor(a.x+t*(b.x-a.x))+','+Math.floor(a.y+t*(b.y-a.y))));
        if(!clear(p,goal)){
          // Route only inside the visible ruin; moving straight at a Bearer can press into a pillar indefinitely.
          const ox=Math.floor(p.x)-20,oy=Math.floor(p.y)-20,local=q=>({x:q.x-ox,y:q.y-oy});
          const map={width:41,height:41,blocked:blockers.map(o=>[Math.floor(o.x)-ox,Math.floor(o.y)-oy]).filter(([x,y])=>x>=0&&x<41&&y>=0&&y<41)};
          const {points}=route(map,local(p),local(goal),{reach:2});
          const steps=points.map(q=>({x:q.x+ox,y:q.y+oy}));
          waypoint=steps.find(q=>Math.hypot(q.x-p.x,q.y-p.y)>.25)??steps.at(-1);
        }else if(Math.hypot(goal.x-p.x,goal.y-p.y)<range)return;
      }else if(Math.hypot(goal.x-p.x,goal.y-p.y)<range)return;
      const dx=waypoint.x-p.x,dy=waypoint.y-p.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');
      if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      const distance=keys.length===2?Math.min(Math.abs(x),Math.abs(y))*Math.SQRT2:Math.max(Math.abs(x),Math.abs(y));
      await actor.act({keys,duration_ms:Math.max(50,Math.min(250,Math.round(distance/(4+5.6*view.character.stats.speed/75)*1000)))});
    }
    throw Error('Native walking did not reach the observed target');
  }
  async function fire(actor,id,ms=150,aroundPillars=false){
    await protect(actor);
    await approach(actor,id,aroundPillars?2.4:1.5,aroundPillars);
    return actor.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
  }
  const initial=await save('idle');await protect(game);
  game.check('untouched naturally placed Lich starts vulnerable at selected HP',entity(initial,root.entity)?.hp===300000&&!(entity(initial,root.entity).condition&(1<<24)));
  if(index<2){
    // Natural ruins can wall off any one side of the Lich's spawn tile corner.
    const moved=async()=>{const v=await game.observe();return Math.hypot(v.position.x-initial.view.position.x,v.position.y-initial.view.position.y)>.3;};
    for(const key of ['s','d','w','a'])if(!await moved())await game.act({keys:[key],duration_ms:150});
    game.check('ordinary movement works at the natural ruin',await moved());
    await approach(game,initial.view.position,.3);
  }
  for(let n=0;n<(index>=2?2:0);n++){
    const partner=await game.join({...spec,seed:794+n,class:n===1?0x31f:spec.class,stats:n===1?[770,252,50,25,50,50,40,75]:spec.stats,position:initial.view.position,equipment:[0xb0b,n===1?0xb26:0xb29,0xa7b,0xacc]});
    partners.push(partner);await protect(partner);
  }
  if(index>=2)for(const actor of [game,...partners])await approach(actor,root.entity,2.5);
  if(index>0)for(const actor of [game,...partners])await cheat(actor,{kind:'stat_modifier',index:2,amount:index===2?50:100,duration_ms:60000});
  for(let n=0;n<20&&phase(await game.inspect())!=='EvaluationStart1';n++){
    if(index>=2)await game.act({keys:[' '],duration_ms:150,target:{entity:root.entity,camera_angle:-Math.PI/4,fire:true}});
    // Solo needs the evaluation to end above 98.5% HP (actors.xml:3245-3246), so admit it with short single swings.
    else await fire(game,root.entity,index===0?50:150);
  }
  let state=await save('admitted');
  game.check('native projectile damage admits the shielded evaluation introduction',phase(state)==='EvaluationStart1'&&!!(entity(state,root.entity).condition&(1<<24)));
  if(index>0){
    // The vulnerable evaluation lasts at most six seconds. Use the observed state and HP
    // instead of assuming the same aim/hold damages a wandering Lich on every tick.
    for(let n=0;n<9&&phase(await game.inspect())==='EvaluationStart1';n++)await idle(300);
    const goal=[0,292000,277000,259000][index];
    if(index>=2)await game.act({keys:['1'],duration_ms:100});
    for(let n=0;n<12;n++){
      const frame=await save('evaluation-pulse-'+n),e=entity(frame,root.entity);
      if(phase(frame)!=='EvaluationStart2'||e.hp<=goal)break;
      const actors=[game,...partners];
      await game.act([actors.map(actor=>({actor:actor.actor,input:{
        keys:n===0?[' ']:[],duration_ms:index===3?700:400,
        target:{entity:root.entity,camera_angle:-Math.PI/4,fire:true},
      }}))]);
    }
    if(index>=2)game.check('one native Dexterity effusion supplies the evaluation fixture',(await game.observe()).character.stats.dexterity===93);
  }
  // Solo uses the full XML 2.5 s shield + 6 s evaluation (actors.xml:3224–3246).
  // Stop on the observed branch, not on an assumed amount of wall time.
  for(let n=0;n<(index===0?20:12);n++){
    const frame=await game.inspect();
    if(!['EvaluationStart1','EvaluationStart2','EvaluationEnd'].includes(phase(frame)))break;
    await idle(index===0?500:200);
  }
  state=await save('selected');
  // Tossed Bearers can heal before the browser yields the first snapshot of this state.
  const lost=Object.values(entity(state,root.entity)?.damage??{}).reduce((a,b)=>a+b,0);
  game.check(branch+' selected from observed combat damage',phase(state)===branch&&lost>[3,4500,14400,37500][index]&&(index===3||lost<=[4500,14400,37500][index]));
  const selected=entity(state,root.entity),xp=state.view.character.experience;
  game.check('preparation is vulnerable and transformation has not awarded XP',!(selected.condition&(1<<24))&&xp===initial.view.character.experience);
  if(index>0){
    const bearerCount=s=>family(s).filter(e=>e.kind===0x91d).length;
    for(let n=0;n<5&&bearerCount(state)<[2,3,5][index-1];n++){await idle(450);state=await save('toss-landing-'+n);}
    game.check('owned bearer first wave matches branch',[2,3,5][index-1]===bearerCount(state));
    for(let n=0;n<8&&entity(state,root.entity)?.hp<300000;n++){await idle(350);state=await save('healing-'+n);}
    game.check('living owned bearers restore their damaged Lich',entity(state,root.entity)?.hp===300000&&lost>4500);
    if(index===1){
      for(let n=0;n<35&&!family(await game.inspect()).some(e=>e.kind===0x91e);n++)await idle(500);
      state=await save('spirit-shield');const spirit=family(state).find(e=>e.kind===0x91e);assert(spirit,'SmallGroup2 must summon an owned Haunted Spirit');
      game.check('summoned Spirit starts invulnerable',!!(spirit.condition&(1<<24)));
      await idle(Math.max(0,spirit.activated+7500-(await game.observe()).game_ms));state=await save('spirit-vulnerable');
      game.check('same Spirit becomes vulnerable during its attack window',!(entity(state,spirit.id).condition&(1<<24)));
    }
  }
  for(let n=0;n<22&&entity(await game.inspect(),root.entity);n++){await idle(2000);if(index>=2)await save('preparation-'+n);}
  state=await save('actual');const actual=family(state).find(e=>e.kind===0x91c);assert(actual,'Actual Lich required');
  game.check('transformation preserves encounter ownership without XP',!entity(state,root.entity)&&actual.quest===root.entity&&state.view.character.experience===xp);
  await approach(game,actual.id,2.5);
  if(index>=2){
    const births=[...new Set(records.flatMap(r=>family(r.state)).filter(e=>e.kind===0x91d).map(e=>e.id))];
    game.check('both owned Bearer waves are admitted',births.length===(index===2?6:8));
    game.check('owned Spirits respect the encounter live cap',family(state).filter(e=>e.kind===0x91e).length>0&&family(state).filter(e=>e.kind===0x91e).length<=(index===2?2:5));
    // The eight-entry observe summary can be full of Mummies and Bearers even when the Lich is in the client view.
    const actualInFrame=({view})=>view.objects.some(e=>e.id===actual.id&&e.kind===0x91c&&Math.hypot(e.position.x-view.position.x,e.position.y-view.position.y)<6);
    game.check('exact Actual Lich is in the native scene before capture',actualInFrame(await save('capture-before')));
    await game.act({keys:[],duration_ms:750},{name:branch+'-actual',when:view=>view.lifecycle.alive});
    game.check('exact Actual Lich remains in the native scene after capture',actualInFrame(await save('capture-after')));
    const bearer=family(await game.inspect()).find(e=>e.kind===0x91d);assert(bearer);
    await fire(game,bearer.id,1000);state=await save('bearer-damaged');
    game.check('ordinary native fire damages an owned bearer',entity(state,bearer.id)?.hp<6000);
    game.check('native group remains alive after branch verification',(await Promise.all([game,...partners].map(actor=>actor.observe()))).every(view=>view.lifecycle.alive));
    await writeFile(join(out,branch+'-branch.json'),JSON.stringify({branch,root,actual,births,state}));
    return;
  }
  await game.act({keys:[],duration_ms:750},{name:branch+'-actual',when:view=>view.nearby.some(e=>e.kind===0x91c)});
  await idle(16000);state=await save('mummies');
  game.check('all three owned mummy variants coexist',[0x65f,0x660,0x661].every(k=>family(state).some(e=>e.kind===k)));
  const samples=[];
  // The Actual Lich wanders and its Shoots use the default range 8 (actors.xml:3314, compile.rs Shoot defaults), so keep closing in
  // while sampling; ruins can block a straight line (VPS 705420bf), and the attack checks below stay strict.
  for(let n=0;n<20;n++){
    for(let k=0;k<4;k++){
      const v=await game.observe(),p=entity(await game.inspect(),actual.id)?.pos;assert(p,'Living Actual Lich required');
      const dx=p.x-v.position.x,dy=p.y-v.position.y;if(Math.hypot(dx,dy)<5)break;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await protect(game);await game.act({keys,duration_ms:200});
    }
    await idle(1000);samples.push(await game.inspect());
  }
  await writeFile(join(out,'attacks.json'),JSON.stringify(samples));
  for(const [kind,speed,lifetime] of [[0x65f,70,3000],[0x660,100,2000],[0x661,70,3000]]){
    game.check(kind+': native scene contains its distinct selected projectile',samples.some(s=>s.view.projectiles.some(p=>family(s).some(e=>e.kind===kind&&e.id===p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime)));
  }
  for(const speed of [80,60])game.check('Actual Lich fires projectile speed '+speed,samples.some(s=>s.view.projectiles.some(p=>p.owner===actual.id&&p.speed===speed)));
  game.check('Actual Lich moves while attacking',samples.some(s=>{const e=entity(s,actual.id);return e&&Math.hypot(e.pos.x-actual.pos.x,e.pos.y-actual.pos.y)>.2;}));
  state=await save('attacks-observed');
  game.check('encounter stays within per-kind population bounds',[[0x65f,4],[0x660,2],[0x661,1],[0x91d,8],[0x91e,[0,1,2,5][index]]].every(([k,max])=>family(state).filter(e=>e.kind===k).length<=max));
  if(index>0){
    const bearer=family(state).find(e=>e.kind===0x91d);assert(bearer);await fire(game,bearer.id,150);await idle(500);state=await save('bearer-damaged');
    game.check('native sword fire damages a vulnerable bearer',entity(state,bearer.id)?.hp<6000);
  }
  for(let n=0;n<60;n++){
    state=await game.inspect();const bearer=family(state).find(e=>e.kind===0x91d);if(!bearer)break;await fire(game,bearer.id,250,true);
  }
  game.check('native combat defeats every owned healer',!family(await game.inspect()).some(e=>e.kind===0x91d));
  // Aim again after each volley and keep a clear lane past the ruin's pillars.
  for(let n=0;n<30&&entity(await game.inspect(),actual.id);n++)await fire(game,actual.id,700,true);
  state=await save('defeated');
  game.check('Actual Lich is defeated by native sword fire',!entity(state,actual.id));
  game.check('eligible player receives defeat experience',state.view.character.experience>xp);
  const earned=state.view.character.experience;await idle(2000);
  game.check('defeat credit is once only',(await game.observe()).character.experience===earned);
  state=await save('rewards');
  const reward=await journaledReward(config,actual.id,state.view.session.character);
  game.check('journaled Actual Lich allocation follows its XML private rows for the eligible contributor',reward.valid&&reward.share>=.1);
  const bags=state.diagnostic.entities.filter(e=>e.items?.some(i=>i!==-1)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===state.view.session.account)&&Math.hypot(e.pos.x-reward.position.x,e.pos.y-reward.position.y)<1);
  await writeFile(join(out,branch+'-reward.json'),JSON.stringify({reward,bags}));
  let picked=state;
  if(!reward.items.length)game.check('empty journaled roll publishes no private Lich bag',!bags.length);
  else{
  const bag=bags.find(b=>JSON.stringify(b.items.filter(i=>i!==-1).sort())===JSON.stringify([...reward.items].sort()));assert(bag,'Journaled Lich allocation must publish its private bag');
  await approach(game,bag.pos,.4);
  await game.act({keys:[],duration_ms:700},{name:branch+'-reward',when:()=>true});
  const inventory=(await game.observe()).character,slot=bag.items.findIndex(i=>i!==-1),item=bag.items[slot],destination=inventory.items.findIndex((v,n)=>n>=4&&v===-1);assert(destination>=4,'Free inventory slot required');
  const to={x:634+(destination%4)*44,y:428+Math.floor((destination-4)/4)*44};
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to}});
  picked=await save('picked');
  game.check('native drag commits one eligible reward',picked.view.character.items[destination]===item&&picked.view.character.inventory_revision===inventory.inventory_revision+1&&(!entity(picked,bag.id)||entity(picked,bag.id).items[slot]===-1));
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to:{x:678,y:428}}});
  game.check('empty-slot drag cannot duplicate the reward',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  }
  await game.act({keys:['Insert'],duration_ms:250});
  const retained=await game.observe();
  game.check('Nexus reconnect preserves the living character and collected reward',retained.world==='Nexus'&&retained.lifecycle.alive&&retained.session.character===initial.view.session.character&&JSON.stringify(retained.character.items)===JSON.stringify(picked.view.character.items)&&retained.character.inventory_revision===picked.view.character.inventory_revision);
  await writeFile(join(out,branch+'-branch.json'),JSON.stringify({branch,root,actual,earned,picked,retained}));
}
