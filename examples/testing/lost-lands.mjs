import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';
const out=process.env.QA_RESULTS_DIR;
const spec={class:0x31d,level:20,seed:817,equipment:[0xb0b,0xb29,0xafc,0xac0],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
const history=async()=>(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
let sites,refuge,reserved,world;
export async function prepare(api){
  const events=await history(),ends=events.flatMap((f,i)=>f.message==='Realm simulation populated'?[i]:[]),candidates=[];
  assert(ends.length===2,'Two initial ordinary Realms required');
  for(const [index,id] of [-3,1].entries()){
    const info=await api({op:'world_info',world:id}),batch=events.slice(index===0?0:ends[index-1]+1,ends[index]);
    assert(info.realm_layout===events[ends[index]].layout,'World and population log must agree');
    candidates.push({world:id,info,sites:batch.filter(f=>f.message==='finite named Realm quest placed'&&[0x675,0x678,0x668,0x67d].includes(f.kind)),reserved:batch.filter(f=>f.message==='Realm event reserved outside active indexes'&&f.kind===0xd50)});
  }
  candidates.sort((a,b)=>b.sites.length-a.sites.length);
  ({sites,reserved,world}=candidates[0]);
  const [x,y]=candidates[0].info.spawn_points[0];refuge={x,y};
  assert(reserved.length===1,'One Lord reserve per ordinary Realm required');
  assert(sites.length,'Ordinary quest sites required for event admission');
  sites.sort((a,b)=>[0x675,0x678,0x668,0x67d].indexOf(a.kind)-[0x675,0x678,0x668,0x67d].indexOf(b.kind));
  await writeFile(join(out,'sites.json'),JSON.stringify({candidates,world,sites}));
  return {...spec,world,position:{x:sites[0].x,y:sites[0].y}};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const records=[];let until=0,modifiedUntil=0,scoutUntil=0,lastTeleport=-10000;
  const browser=config.native.find(n=>n.actor===game.actor),connection=await connect({...browser,url:config.origin+'/',helper:config.browserTools});
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command})});
    const receipt=await response.json();assert(response.ok&&(command.op==='inspect'?receipt.diagnostic!==undefined:receipt.status==='succeeded'),JSON.stringify(receipt));return receipt;
  }
  const scout=(await api({op:'spawn',spec:{...spec,world,guid:'lost-lands-scout',position:refuge,controller:'direct',profile:null}})).result.actor;
  const scoutName=(await api({op:'inspect',actor:scout})).diagnostic.character.name;
  try{
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  async function save(label){const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounter.json'),JSON.stringify(records));return state;}
  async function cheat(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function protect(){const now=(await game.observe()).game_ms;if(now>=until){await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});until=now+60000;}}
  async function idle(ms){while(ms>0){await protect();const step=Math.min(ms,5000,until-(await game.observe()).game_ms);await game.act({keys:[],duration_ms:step});ms-=step;}}
  async function at(point){
    const now=(await game.observe()).game_ms;
    if(now-lastTeleport<10000)await idle(10000-(now-lastTeleport));
    await protect();
    if((await game.observe()).game_ms>=scoutUntil){await api({op:'cheat',actor:scout,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});scoutUntil=(await game.observe()).game_ms+60000;}
    await api({op:'cheat',actor:scout,change:{kind:'teleport',position:point}});
    const action=game.act({keys:[],duration_ms:2000});
    const typing=(async()=>{
      for(let n=0;n<100&&(await game.observe()).paused;n++)await new Promise(ok=>setTimeout(ok,10));
      async function enter(){try{await connection.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}finally{await connection.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}}
      await enter();await connection.call('Input.insertText',{text:'/teleport '+scoutName});await enter();
    })();
    for(const r of await Promise.allSettled([action,typing]))if(r.status==='rejected')throw r.reason;
    const arrived=await game.observe();assert(Math.hypot(arrived.position.x-point.x,arrived.position.y-point.y)<.2,'Native teleport must reach direct scout');lastTeleport=arrived.game_ms;
    await api({op:'cheat',actor:scout,change:{kind:'teleport',position:refuge}});
  }
  async function walk(point){
    for(let n=0;n<30;n++){
      await protect();const v=await game.observe(),dx=point.x-v.position.x,dy=point.y-v.position.y;if(Math.hypot(dx,dy)<1.5)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.max(50,Math.min(500,Math.round(Math.hypot(dx,dy)/10*1000)))});
    }
    throw Error('Native walk did not reach target');
  }
  async function fire(id,ms=500){
    await protect();const state=await game.inspect(),target=entity(state,id);assert(target,'Living target required');
    if(Math.hypot(target.pos.x-state.view.position.x,target.pos.y-state.view.position.y)>2)await walk(target.pos);
    const now=(await game.observe()).game_ms;if(now>=modifiedUntil){await cheat({kind:'stat_modifier',index:2,amount:100,duration_ms:60000});modifiedUntil=now+60000;}
    await game.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
  }
  const initial=await save('before-admission');
  game.check('startup reserves one Lord outside active indexes',reserved.length===1);
  let admitted;
  for(const site of sites){
    await at({x:site.x,y:site.y});
    for(let n=0;n<120;n++){
      const state=await game.inspect(),root=entity(state,site.entity),final=state.diagnostic.entities.find(e=>e.quest===site.entity&&[0x6c2,0x6c1].includes(e.kind));
      const target=root??final;if(!target)break;
      await fire(target.id,2000);
    }
    await idle(1000);await save('quest-'+site.entity);
    const events=(await history()).filter(f=>f.message==='quest defeat summoned a Realm event');
    admitted=events.find(f=>f.kind===0xd50);
    if(admitted)break;
    for(const event of events.filter(f=>[0xd56,0xd59].includes(f.kind)))if(!sites.some(s=>s.entity===event.entity))sites.push(event);
  }
  assert(admitted,'Bounded ordinary quest defeats did not admit a Lord; do not substitute a spawned fixture');
  await writeFile(join(out,'admission.json'),JSON.stringify(admitted));
  await at({x:admitted.x,y:admitted.y});
  await game.act({keys:[],duration_ms:500},{name:'lost-lands-waiting',when:()=>true});
  const entry=await save('entry'),lord=entity(entry,admitted.entity);assert(lord?.hp===70000&&lord.stage===0);
  game.check('ordinary quest defeat admits the untouched reserved Lord',lord.id===reserved[0].entity&&lord.kind===0xd50&&lord.quest===lord.id);
  await fire(lord.id,500);await idle(4000);
  let state=await save('activated');
  const family=s=>s.diagnostic.entities.filter(e=>e.quest===lord.id),crystals=s=>family(s).filter(e=>e.kind===0xd51);
  game.check('native damage activates Lord and summons Guardians and Knights',entity(state,lord.id).hp<69300&&[0xd52,0xd53].every(k=>family(state).some(e=>e.kind===k)));
  const samples=[];let pending,error;
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>samples.push(s)).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
  try{await idle(23000);}finally{clearInterval(timer);if(pending)await pending;}if(error)throw error;
  await writeFile(join(out,'attacks.json'),JSON.stringify(samples));state=await save('protected');
  game.check('Lord uses White Beam and Bleeding Vortex projectiles',[83,35].every(speed=>samples.some(s=>s.view.projectiles.some(p=>p.owner===lord.id&&p.speed===speed))));
  game.check('Lord moves during its attack',samples.some(s=>{const e=entity(s,lord.id);return e&&Math.hypot(e.pos.x-lord.pos.x,e.pos.y-lord.pos.y)>.2;}));
  game.check('eight owned protection crystals shield Lord',crystals(state).length===8&&entity(state,lord.id).stage===8&&!!(entity(state,lord.id).condition&(1<<24)));
  const hp=entity(state,lord.id).hp;await fire(lord.id,1000);state=await save('shield-rejects');
  game.check('shield rejects native sword damage',entity(state,lord.id).hp===hp);
  for(const crystal of crystals(state))for(let n=0;n<35&&entity(await game.inspect(),crystal.id);n++)await fire(crystal.id);
  await idle(500);state=await save('shield-released');
  game.check('defeating all crystals returns Lord to combat',!crystals(state).length&&entity(state,lord.id).stage!==8&&!(entity(state,lord.id).condition&(1<<24)));
  const beforeFinalXp=state.view.character.experience;
  for(let n=0;n<300&&entity(await game.inspect(),lord.id);n++){
    state=await game.inspect();
    const crystal=crystals(state)[0];
    await fire(crystal?.id??lord.id,1000);
  }
  await idle(1000);state=await save('defeated');
  game.check('native combat completes timed Lord defeat and earns XP',!entity(state,lord.id)&&state.view.character.experience>=beforeFinalXp+2100&&(await history()).some(f=>f.message==='timed enemy defeat'&&f.entity===lord.id));
  const xp=state.view.character.experience;await idle(2000);
  game.check('defeat XP is once only',(await game.observe()).character.experience===xp);
  game.check('same living prepared character survives',state.view.lifecycle.alive&&state.view.session.character===initial.view.session.character);
  await game.act({keys:[],duration_ms:500},{name:'lost-lands-defeated',when:()=>true});
  }finally{connection.close();}
}
