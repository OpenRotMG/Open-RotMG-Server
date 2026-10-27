import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x307,level:20,seed:798,world:1,equipment:[0xc02,0xade,0xaf9,0xacd],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50]};
export async function prepare(api){
  const registry=JSON.parse(await readFile(resolve('data/realm-sites.json'),'utf8'));
  for(const id of [1,2]){
    const world=await api({op:'world_info',world:id}),record=registry.find(r=>r.layout===world.realm_layout);
    if(!record)continue;
    const site=record.sites.find(s=>s.kind===0x67d);
    if(!site)throw Error('Authored Realm has no registered Castle');
    const boss=world.encounter_markers.find(e=>e.kind===0x67d&&e.position.x===site.spawn[0]&&e.position.y===site.spawn[1]);
    if(!boss)throw Error('Registered Castle has no ordinary Cyclops God');
    await writeFile(join(process.env.QA_RESULTS_DIR,'site.json'),JSON.stringify({world,boss,site}));
    return {...spec,world:id,position:{x:boss.position.x+2,y:boss.position.y+2}};
  }
  throw Error('Neither active Realm has an authored Castle layout; this case requires one');
}
export async function run(game){
  let serial=0;
  async function cheat(change){
    const path=join(process.env.QA_RESULTS_DIR,'preparation-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'cyclops-preparation-'+serial++,command:{op:'cheat',actor:game.actor,change}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);
  }
  const save=async name=>{const state=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(state));return state;};
  async function fire(id){
    const view=(await game.inspect()).view,e=view.objects.find(e=>e.id===id);
    if(!e)throw Error('Requested bow target is not visible');
    const dx=e.position.x-view.position.x,dy=e.position.y-view.position.y,x=50*(dx-dy)/Math.SQRT2,y=50*(dx+dy)/Math.SQRT2,scale=Math.max(1,Math.hypot(x,y)/270);
    await game.act({keys:[],duration_ms:500,mouse:{x:300+x/scale,y:325+y/scale}});
  }
  await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});
  await game.act({keys:[],duration_ms:3500});
  await game.act({keys:[],duration_ms:500},{name:'cyclops-entry',when:()=>true});
  const first=await save('initial'),boss=first.diagnostic.entities.find(e=>e.kind===0x67d),family=[0x67e,0x67f,0x680,0x681,0x682];
  game.check('naturally placed Realm boss activates',!!boss&&boss.activated!==null);
  const children=state=>state.diagnostic.entities.filter(e=>e.parent===boss.id&&family.includes(e.kind));
  game.check('all five dependent species spawn',family.every(k=>children(first).some(e=>e.kind===k)));
  const speeds=new Set(),stages=new Set(),trace=[];
  // Comparator differences are research, not assertions of historical fidelity: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Cyclops.cs
  function record(state){
    for(const p of state.view.projectiles.filter(p=>p.owner===boss.id))speeds.add(p.speed);
    for(const e of children(state))stages.add(e.stage);
    trace.push({time:state.view.time_ms,boss:state.diagnostic.entities.find(e=>e.id===boss.id)?.pos,children:children(state).map(e=>({id:e.id,kind:e.kind,stage:e.stage,pos:e.pos}))});
  }
  let pending=null,sampleError=null;
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{sampleError=e;}).finally(()=>{pending=null;});},100);
  try{await game.act({keys:[],duration_ms:10000},{name:'cyclops-attacks',when:()=>true});}
  finally{clearInterval(timer);if(pending)await pending;}
  if(sampleError)throw sampleError;
  await writeFile(join(process.env.QA_RESULTS_DIR,'cycle.json'),JSON.stringify({speeds:[...speeds],stages:[...stages],trace}));
  game.check('native session observes blades and all four beam speeds',[65,20,40,60,80].every(s=>speeds.has(s)));
  const distance=e=>Math.hypot(e.pos.x-first.view.position.x,e.pos.y-first.view.position.y),motions=new Set();
  for(let n=1;n<trace.length;n++)for(const e of trace[n].children){
    const prior=trace[n-1].children.find(p=>p.id===e.id),sign=e.stage===0?-1:1;
    if(prior&&prior.stage===e.stage&&sign*(distance(e)-distance(prior))>.02)motions.add(e.kind+':'+e.stage);
  }
  game.check('all five species visibly pursue and retract',family.every(k=>[0,1].every(s=>motions.has(k+':'+s))));
  game.check('boss wanders in its castle',trace.some(t=>t.boss&&Math.hypot(t.boss.x-boss.pos.x,t.boss.y-boss.pos.y)>.2));
  const before=await game.observe();
  await game.act({keys:['d'],duration_ms:300});
  game.check('ordinary native movement works',Math.hypot((await game.observe()).position.x-before.position.x,(await game.observe()).position.y-before.position.y)>.5);
  await game.act({keys:['a'],duration_ms:300});
  const scene=await game.inspect(),victim=children(scene).find(e=>scene.view.objects.some(o=>o.id===e.id&&o.hp>0));
  if(!victim)throw Error('No visible follower available for replenishment check');
  const prior=new Set(children(scene).map(e=>e.id));
  for(let n=0;n<12&&(await game.inspect()).view.objects.some(e=>e.id===victim.id&&e.hp>0);n++)await fire(victim.id);
  await game.act({keys:[],duration_ms:300});
  game.check('native bow fire defeats a follower',!(await game.inspect()).diagnostic.entities.some(e=>e.id===victim.id));
  for(let n=0;n<25&&!children(await game.inspect()).some(e=>e.kind===victim.kind&&!prior.has(e.id));n++)await game.act({keys:[],duration_ms:1000});
  const replenished=await save('replenished');
  game.check('living boss replenishes defeated species',children(replenished).some(e=>e.kind===victim.kind&&!prior.has(e.id)));
  game.check('sampled population respects current three-per-species cap',family.every(k=>children(replenished).filter(e=>e.kind===k).length<=3));
  const xp=(await game.observe()).character.experience;
  for(let n=0;n<30&&(await game.inspect()).view.objects.some(e=>e.id===boss.id&&e.hp>0);n++)await fire(boss.id);
  await game.act({keys:[],duration_ms:300});
  const defeated=await save('defeated');
  game.check('native bow fire defeats the same boss and earns XP',!defeated.diagnostic.entities.some(e=>e.id===boss.id)&&defeated.view.character.experience>xp);
  await game.act({keys:[],duration_ms:500},{name:'cyclops-defeated',when:()=>true});
  game.check('same living character retains supplied equipment',defeated.view.lifecycle.alive&&defeated.view.session.character===first.view.session.character&&JSON.stringify(defeated.view.character.items)===JSON.stringify(first.view.character.items));
  const chest=first.diagnostic.entities.find(e=>e.kind===0x501&&e.items.some(i=>i!==-1));
  if(!chest)throw Error('Natural Castle has no stocked chest');
  game.check('Castle chest predates boss defeat and retains its contents',defeated.diagnostic.entities.some(e=>e.id===chest.id&&JSON.stringify(e.items)===JSON.stringify(chest.items)));
  for(let n=0;n<60;n++){
    const view=await game.observe(),dx=chest.pos.x-view.position.x,dy=chest.pos.y-view.position.y;
    if(Math.hypot(dx,dy)<.4)break;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>.12&&Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');
    if(Math.abs(y)>.12&&Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
    const distance=keys.length===2?Math.min(Math.abs(x),Math.abs(y))*Math.SQRT2:Math.max(Math.abs(x),Math.abs(y));
    await game.act({keys,duration_ms:Math.max(50,Math.min(300,Math.round(distance/(4+5.6*view.character.stats.speed/75)*1000)))});
  }
  const arrived=await game.observe();
  game.check('ordinary native movement reaches the Castle chest',Math.hypot(arrived.position.x-chest.pos.x,arrived.position.y-chest.pos.y)<.4);
  await game.act({keys:[],duration_ms:700},{name:'cyclops-chest',when:()=>true});
  const inventory=await game.observe(),item=chest.items[0];
  game.check('selected chest first slot is stocked',item!==-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await save('picked');
  game.check('ordinary native drag acquires chest loot exactly once',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===inventory.character.inventory_revision+1&&picked.diagnostic.entities.find(e=>e.id===chest.id)?.items[0]===-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});
  game.check('empty chest slot cannot duplicate loot',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
}

