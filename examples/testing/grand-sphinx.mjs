import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';
const out=process.env.QA_RESULTS_DIR,sources=[0x678,0x67d,0x668,0xd56,0xd59,0x928,0x91b,0x675];let refuge;
const spec={class:0x323,level:20,seed:818,world:1,equipment:[0xb0a,0xb2d,0xb05,0xacd],backpack:Array(8).fill(-1),stats:[670,385,60,25,60,50,40,75]};
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),spawn=world.spawn_points[0];assert(spawn?.length===2);refuge={x:spawn[0],y:spawn[1]};
  return {...spec,position:refuge};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'},observations=[];let protection=0,modifier=0;
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command})});
    const receipt=await response.json();assert(response.ok&&(command.op==='inspect'?receipt.diagnostic!==undefined:receipt.status==='succeeded'),JSON.stringify({op:command.op,status:receipt.status,error:receipt.error}));return receipt;
  }
  const cheat=change=>api({op:'cheat',actor:game.actor,change});
  const scout=(await api({op:'spawn',spec:{...spec,guid:'sphinx-scout',position:refuge,controller:'direct',profile:null}})).result.actor;
  const scoutName=(await api({op:'inspect',actor:scout})).diagnostic.character.name;
  const browser=config.native.find(n=>n.actor===game.actor),connection=await connect({...browser,url:config.origin+'/',helper:config.browserTools});let scoutProtection=0,scoutModifier=0;
  try{
  async function prepareScout(change){
    for(let n=0;n<100;n++){
      const state=(await api({op:'inspect',actor:scout})).view.lifecycle;assert(state.alive&&!state.death_pending&&!state.departing,'Prepared scout must remain alive');
      if(!state.busy)return api({op:'cheat',actor:scout,change});await new Promise(ok=>setTimeout(ok,50));
    }
    throw Error('Scout persistence did not finish before the next prepared mutation');
  }
  async function playing(){
    const deadline=Date.now()+15000;
    while(Date.now()<deadline){if(!(await game.observe()).paused)return;await new Promise(ok=>setTimeout(ok,25));}
    throw Error('Native action did not establish its active bounded window within 15 seconds');
  }
  async function travel(point){
    await protect();
    await prepareScout({kind:'teleport',position:point});
    const action=game.act({keys:[],duration_ms:2000});
    const typing=(async()=>{
      await playing();
      async function enter(){try{await connection.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}finally{await connection.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}}
      await enter();await connection.call('Input.insertText',{text:'/teleport '+scoutName});await enter();
    })();
    const results=await Promise.allSettled([action,typing]);for(const r of results)if(r.status==='rejected')throw r.reason;
    const arrived=await game.observe();assert(Math.hypot(arrived.position.x-point.x,arrived.position.y-point.y)<.2,'Ordinary native chat teleport must reach the prepared scout');
    await prepareScout({kind:'teleport',position:refuge});
  }
  async function protect(){
    const now=(await game.observe()).game_ms;
    if(now>=protection){await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});protection=now+60000;}
    // Only the human tests projectile conditions; the scout is admission preparation.
    if(now>=scoutProtection){await prepareScout({kind:'protection',condition:'invincible',duration_ms:60000});scoutProtection=now+60000;}
  }
  async function idle(ms,capture){
    while(ms>0){await protect();const now=(await game.observe()).game_ms,step=Math.min(ms,10000,protection-now,scoutProtection-now);await game.act({keys:[],duration_ms:step},capture);capture=undefined;ms-=step;}
  }
  async function map(){
    const view=await game.observe(),response=await fetch(config.origin+'/dev/map?player='+view.session.player,{headers});assert(response.ok);const scene=await response.json();
    return {width:scene.width,height:scene.height,entities:scene.entities.filter(e=>e.kind===0xd54||sources.includes(e.kind))};
  }
  async function save(label){const state=await game.inspect();observations.push({label,state,scout:(await api({op:'inspect',actor:scout})).view});await writeFile(join(out,'sphinx.json'),JSON.stringify(observations));return state;}
  const entity=(state,id)=>state.diagnostic.entities.find(e=>e.id===id);
  async function walk(point,range=.4){
    for(let n=0;n<100;n++){
      await protect();const view=await game.observe(),dx=point.x-view.position.x,dy=point.y-view.position.y;if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      const distance=keys.length===2?Math.min(Math.abs(x),Math.abs(y))*Math.SQRT2:Math.max(Math.abs(x),Math.abs(y));
      await game.act({keys,duration_ms:Math.max(50,Math.min(2000,Math.round(distance/(4+5.6*view.character.stats.speed/75)*1000)))});
    }
    throw Error('Native walking did not reach the observed target');
  }
  async function fire(id,ms=2000){
    await protect();const state=await game.inspect(),target=entity(state,id);assert(target,'Living target required');
    if(Math.hypot(target.pos.x-state.view.position.x,target.pos.y-state.view.position.y)>6)await walk(target.pos,5);
    const now=(await game.observe()).game_ms;if(now>=modifier){await cheat({kind:'stat_modifier',index:2,amount:100,duration_ms:60000});modifier=now+60000;}
    await game.act({keys:[],duration_ms:Math.min(ms,protection-now,scoutProtection-now),target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
  }
  async function sourceFire(target){
    const id=target.id;
    await protect();let now=(await game.observe()).game_ms;const remaining=Math.min(protection-now,scoutProtection-now);
    if(remaining<6100){await prepareScout({kind:'teleport',position:refuge});await idle(remaining);await protect();now=(await game.observe()).game_ms;}
    assert(target.hp>0,'Ordinary admission target must be alive');
    await prepareScout({kind:'teleport',position:target.pos});
    if(now>=scoutModifier){await prepareScout({kind:'stat_modifier',index:2,amount:100,duration_ms:60000});scoutModifier=now+60000;}
    const action=game.act({keys:[],duration_ms:6000});
    const firing=(async()=>{
      await playing();
      for(let n=0;n<4;n++){
        const state=(await api({op:'inspect',actor:scout})),enemy=entity(state,id);if(!enemy)break;
        if((await game.observe()).game_ms>now+4500)break;
        await api({op:'act',actor:scout,action:{command:'shoot',angle:Math.atan2(enemy.pos.y-state.view.position.y,enemy.pos.x-state.view.position.x)},duration_ms:1000,pauseAfter:false});
      }
    })();
    const results=await Promise.allSettled([action,firing]);for(const r of results)if(r.status==='rejected')throw r.reason;
  }
  await protect();await game.act({keys:[],duration_ms:100});
  const attempted=new Set();let sphinx;
  for(let n=0;n<100;n++){
    const scene=await map();sphinx=scene.entities.find(e=>e.kind===0xd54);if(sphinx)break;
    const here=(await game.observe()).position;
    const rank=e=>Math.hypot(e.pos.x-here.x,e.pos.y-here.y)+([0x928,0x91b].includes(e.kind)?scene.width+scene.height:0);
    const root=scene.entities.filter(e=>sources.includes(e.kind)&&!attempted.has(e.id)).sort((a,b)=>rank(a)-rank(b))[0];
    assert(root,'No unattempted qualifying Realm source remains');attempted.add(root.id);
    await prepareScout({kind:'teleport',position:root.pos});await protect();
    for(let hit=0;hit<30;hit++){
      const state=await api({op:'inspect',actor:scout}),target=state.diagnostic.entities.filter(e=>(e.id===root.id||e.quest===root.id&&[0x92d,0x91c,0x6c1,0x91d,0x929].includes(e.kind))&&!(e.kind===0x929&&e.stage===3)).sort((a,b)=>Number(![0x91d,0x929].includes(a.kind))-Number(![0x91d,0x929].includes(b.kind)))[0];
      if(!target)break;await sourceFire(target);
    }
    await game.act({keys:[],duration_ms:200});await save('admission-source-'+root.id);
  }
  assert(sphinx,'Ordinary quest deaths did not admit a Sphinx within the bounded search');
  await writeFile(join(out,'admission.json'),JSON.stringify({attempted:[...attempted],preparation_actor:scout,sphinx}));
  await travel(sphinx.pos);await game.act({keys:[],duration_ms:500},{name:'sphinx-entry',when:()=>true});
  const initial=await save('entry'),boss=entity(initial,sphinx.id);assert(boss);
  game.check('unmodified full-health Sphinx admitted by ordinary Realm quest deaths',boss.hp===55000);
  const reapers=initial.diagnostic.entities.filter(e=>e.kind===0xd55&&e.quest===boss.quest);
  game.check('exactly four owned Reapers accompany the sole Sphinx',reapers.length===4&&(await map()).entities.filter(e=>e.kind===0xd54).length===1);
  // Target build removes Reaper Stasis immunity: https://wikiwiki.jp/rmd/version/build123.5.1
  const distance=e=>Math.hypot(e.pos.x-initial.view.position.x,e.pos.y-initial.view.position.y);
  const victim=reapers.slice().sort((a,b)=>distance(a)-distance(b))[0];assert(victim);
  await game.act({keys:[' '],duration_ms:150,target:{entity:victim.id,camera_angle:-Math.PI/4,fire:false}});
  const frozen=await save('reaper-stasis');
  game.check('native Mystic orb Stasises a Reaper without damaging it',entity(frozen,victim.id)?.statuses['22']&&entity(frozen,victim.id).hp===6000);
  const stages=new Set(),speeds=new Set(),trace=[];let pending=null,sampleError;
  async function sample(){const state=await game.inspect(),e=entity(state,boss.id);if(e)stages.add(e.stage);for(const p of state.view.projectiles.filter(p=>p.owner===boss.id))speeds.add(p.speed);trace.push({time:state.view.time_ms,boss:e,reapers:state.diagnostic.entities.filter(e=>e.kind===0xd55&&e.quest===boss.quest)});}
  const timer=setInterval(()=>{if(!pending)pending=sample().catch(e=>{sampleError=e;}).finally(()=>{pending=null;});},100);
  try{
    for(let n=0;n<5;n++)await idle(6000,n===2?{name:'sphinx-attacks',when:()=>true}:undefined);
    const xp=(await game.observe()).character.experience;
    for(let n=0;n<240&&entity(await game.inspect(),boss.id);n++)await fire(boss.id);
    await game.act({keys:[],duration_ms:1500},{name:'sphinx-defeated',when:()=>true});
    const defeated=await save('defeated');
    game.check('native staff fire defeats the same full-health Sphinx and earns XP',!entity(defeated,boss.id)&&defeated.view.character.experience>xp);
    game.check('owned Reapers disappear after boss defeat',!defeated.diagnostic.entities.some(e=>e.kind===0xd55&&e.quest===boss.quest));
    game.check('native session observes all three Sphinx projectile families',[80,55,67].every(s=>speeds.has(s)));
    game.check('native session observes vulnerable attacks and shielded transitions',[1,2,3,4,5,6].every(s=>stages.has(s)));
  }finally{clearInterval(timer);if(pending)await pending;await writeFile(join(out,'cycle.json'),JSON.stringify({stages:[...stages],speeds:[...speeds],trace}));}
  if(sampleError)throw sampleError;
  const defeated=await game.inspect(),bag=defeated.diagnostic.entities.find(e=>e.items?.some(i=>i===0xa34||i===0xa35));assert(bag,'Eligible potion floor required');
  await walk(bag.pos,1);await game.act({keys:[],duration_ms:700},{name:'sphinx-loot',when:()=>true});
  const before=await game.observe(),item=bag.items[0];assert(item!==-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await save('picked');game.check('native drag commits one earned item',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===before.character.inventory_revision+1);
  const portal=defeated.diagnostic.entities.find(e=>e.kind===0x72c);assert(portal,'Sphinx defeat must create Tomb portal');
  await walk(portal.pos,1);await game.act({keys:[],duration_ms:1000},{name:'sphinx-portal',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:580},duration_ms:300});
  const entered=await save('tomb-entry');game.check('earned Tomb portal admits the same living character',entered.view.map.name==='Tomb of the Ancients'&&entered.view.session.character===initial.view.session.character&&entered.view.lifecycle.alive);
  // The client entry card advances only while native gameplay is running.
  await idle(5000);
  await game.act({keys:['d'],duration_ms:500},{name:'tomb-journey',when:()=>true});
  const walked=await save('tomb-walk');game.check('normal native input moves inside the earned dungeon',Math.hypot(walked.view.position.x-entered.view.position.x,walked.view.position.y-entered.view.position.y)>.5);
  await game.act({keys:['Insert'],duration_ms:300});const retained=await save('returned');
  game.check('normal Nexus return retains earned loot',retained.view.map.name==='Nexus'&&retained.view.character.items[4]===item&&retained.view.character.inventory_revision===picked.view.character.inventory_revision);
  }finally{connection.close();}
}
