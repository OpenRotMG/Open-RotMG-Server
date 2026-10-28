import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.env.QA_RESULTS_DIR;
process.env.RUST_LOG='info,openrotmg::world::native_hits=debug';
const spec={class:0x31d,level:20,seed:799,world:1,equipment:[0xb0b,0xb29,0xafc,0xac0],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
let root;
export async function prepare(api){
  const info=await api({op:'world_info',world:1});
  const events=(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  const end=events.findLastIndex(f=>f.message==='Realm simulation populated'&&f.layout===info.realm_layout);
  assert(end>=0,'Completed Realm population required');
  const start=events.slice(0,end).findLastIndex(f=>f.message==='Realm simulation populated');
  root=events.slice(start+1,end).find(f=>f.message==='finite named Realm quest placed'&&f.kind===0x675&&f.layout===info.realm_layout);
  await writeFile(join(out,'site.json'),JSON.stringify({info,root}));
  assert(root,'This ordinary Realm has zero Pyres, a valid population; this case requires a naturally placed Phoenix');
  return {...spec,position:{x:root.x,y:root.y}};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const records=[],protectedUntil=new Map();
  async function save(label){const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounter.json'),JSON.stringify(records));return state;}
  const history=async()=>(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  async function protect(actor,duration_ms=40000){
    const now=(await actor.observe()).game_ms;
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:actor.actor,change:{kind:'protection',condition:'invulnerable',duration_ms}}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
    protectedUntil.set(actor.actor,now+duration_ms);
  }
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id),family=s=>s.diagnostic.entities.filter(e=>e.quest===root.entity),form=(s,kind)=>family(s).find(e=>e.kind===kind);
  async function approach(actor,point,range=1.5){
    for(let n=0;n<60;n++){
      const view=await actor.observe(),dx=point.x-view.position.x,dy=point.y-view.position.y;
      if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');
      if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await actor.act({keys,duration_ms:Math.max(50,Math.min(200,Math.round(Math.hypot(dx,dy)/10*1000)))});
    }
    throw Error('Native walking did not reach the observed target');
  }
  async function fire(actor,id,ms=150){
    const target=entity(await actor.inspect(),id);assert(target,'Living combat target required');
    await approach(actor,target.pos);
    await actor.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
  }
  const initial=await save('entry');assert(entity(initial,root.entity)?.kind===0x675,'Ordinary placed Lord required');
  await protect(game);
  await game.act({keys:['s'],duration_ms:150});
  game.check('native movement at the naturally placed Pyre',Math.hypot((await game.observe()).position.x-initial.view.position.x,(await game.observe()).position.y-initial.view.position.y)>.3);
  await game.act({keys:['w'],duration_ms:150});
  await game.act({keys:[],duration_ms:1000},{name:'phoenix-lord',when:()=>true});
  const active=await save('active');
  game.check('Lord activates and summons both Birdman kinds',entity(active,root.entity).activated!==null&&[0x676,0x677].every(k=>family(active).some(e=>e.kind===k)));
  game.check('Lord fires selected Fire Bolts',active.view.projectiles.some(p=>p.owner===root.entity&&p.speed===60));
  for(let n=0;n<30&&entity(await game.inspect(),root.entity);n++)await fire(game,root.entity);
  let state=await save('egg-born'),egg=form(state,0x6c2);assert(egg,'Native Lord defeat must produce Egg');
  const lordHistory=await history();
  game.check('Lord transforms without an intermediate defeat or private reward',lordHistory.some(f=>f.message==='named quest transformed without rewards'&&f.entity===root.entity&&f.replacement===egg.id)&&!lordHistory.some(f=>f.message==='enemy defeated'&&f.entity===root.entity)&&!state.diagnostic.entities.some(e=>e.extra_stats.some(([stat,account])=>stat===54&&account>=0)));
  game.check('Egg retains encounter and contributor damage',egg.quest===root.entity&&egg.damage[game.actor]===3000);
  const shield=1<<24,eggHp=egg.hp,eggSize=egg.size;
  game.check('new Egg is invulnerable',!!(egg.condition&shield));
  await game.act({keys:[],duration_ms:300,target:{entity:egg.id,camera_angle:-Math.PI/4,fire:true}},{name:'phoenix-egg-shielded',when:()=>true});
  state=await save('egg-shielded');egg=entity(state,egg.id);
  game.check('shielded Egg rejects native sword damage',egg.hp===eggHp);
  game.check('shielded Egg retains selected size130',eggSize===130&&egg.size===130);
  await game.act({keys:[],duration_ms:2000});
  state=await save('egg-vulnerable');egg=entity(state,egg.id);
  game.check('Egg becomes vulnerable and stays stationary',!(egg.condition&shield)&&Math.hypot(egg.pos.x-form(records.find(r=>r.label==='egg-born').state,0x6c2).pos.x,egg.pos.y-form(records.find(r=>r.label==='egg-born').state,0x6c2).pos.y)<.01);
  for(let n=0;n<45&&entity(await game.inspect(),egg.id);n++)await fire(game,egg.id);
  state=await save('reborn');const reborn=form(state,0x6c1);assert(reborn,'Native Egg defeat must produce Reborn');
  const eggHistory=await history();
  game.check('Egg transforms without an intermediate defeat and preserves damage',eggHistory.some(f=>f.message==='named quest transformed without rewards'&&f.entity===egg.id&&f.replacement===reborn.id)&&!eggHistory.some(f=>f.message==='enemy defeated'&&f.entity===egg.id)&&reborn.damage[game.actor]===8000);
  const shots=[],positions=[];let pending=null,error=null;
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>{shots.push(...s.view.projectiles.filter(p=>p.owner===reborn.id));positions.push(entity(s,reborn.id)?.pos);}).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
  try{await game.act({keys:[],duration_ms:9500},{name:'phoenix-reborn',when:()=>true});}finally{clearInterval(timer);if(pending)await pending;}
  if(error)throw error;
  await writeFile(join(out,'attacks.json'),JSON.stringify({shots,positions}));
  game.check('Reborn exposes both spinner and bolt attacks',shots.some(p=>p.speed===70)&&shots.some(p=>p.speed===60));
  const spinner=shots.find(p=>p.speed===70);assert(spinner,'Observed spinner required');
  const phase=(spinner.angle*180/Math.PI%30+30)%30;
  game.check('first spinner burst uses the centered10-degree fan',Math.abs(phase-25)<.01);
  game.check('Reborn wanders',positions.some(p=>p&&Math.hypot(p.x-reborn.pos.x,p.y-reborn.pos.y)>.2));
  const expires=Math.max(...protectedUntil.values())+50;
  while((await game.observe()).game_ms<expires)await game.act({keys:[],duration_ms:Math.min(10000,expires-(await game.observe()).game_ms)});
  const beforeHit=(await game.observe()).character.hp;
  for(let n=0;n<25&&!((await game.observe()).character.conditions&(1<<2));n++){
    await approach(game,entity(await game.inspect(),reborn.id).pos,.25);
    await game.act({keys:[],duration_ms:350});
  }
  const weakened=await save('weak');
  const events=await history();
  game.check('unprotected native spinner collision applies Weak and loses HP',!!(weakened.view.character.conditions&(1<<2))&&weakened.view.character.hp<beforeHit&&events.some(f=>f.message==='player damaged'&&f.player===weakened.view.session.player&&f.owner===reborn.id&&f.source_kind===0x6c1&&f.damage>0));
  await game.act({keys:[],duration_ms:100},{name:'phoenix-weak',when:()=>true});
  await protect(game,60000);
  const companion=await game.join({...spec,world:initial.view.session.world,seed:800,position:entity(await game.inspect(),reborn.id).pos});
  await protect(companion,60000);
  const xp=await game.observe(),companionXp=await companion.observe();
  await fire(companion,reborn.id,300);
  for(let n=0;n<45&&entity(await game.inspect(),reborn.id);n++)await fire(game,reborn.id);
  await game.act({keys:[],duration_ms:500},{name:'phoenix-defeated',when:()=>true});
  state=await save('defeated');
  game.check('native combat defeats the final Reborn',!entity(state,reborn.id)&&![0x675,0x6c2,0x6c1].some(k=>form(state,k)));
  const earned=await Promise.all([game.observe(),companion.observe()]);
  game.check('both native contributors receive final XP',earned.every((v,i)=>v.character.experience>[xp,companionXp][i].character.experience));
  await game.act({keys:[],duration_ms:1500});
  game.check('final XP is once only',(await game.observe()).character.experience===earned[0].character.experience);
  game.check('same prepared living character and equipment survive',state.view.lifecycle.alive&&state.view.session.character===initial.view.session.character&&JSON.stringify(state.view.character.items)===JSON.stringify(initial.view.character.items));
  await save('settled');
  const chest=initial.diagnostic.entities.find(e=>e.kind===0x501&&e.items.some(i=>i!==-1));assert(chest,'Natural Pyre chest required');
  game.check('public Pyre chest predates combat and retains its contents',entity(state,chest.id)&&JSON.stringify(entity(state,chest.id).items)===JSON.stringify(chest.items));
  await approach(game,chest.pos,.35);
  await game.act({keys:[],duration_ms:600},{name:'phoenix-chest',when:()=>true});
  const inventory=(await game.observe()).character,item=chest.items[0];assert(item!==-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await save('picked');
  game.check('native drag acquires the public chest item exactly once',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===inventory.inventory_revision+1&&entity(picked,chest.id)?.items[0]===-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});
  game.check('empty chest slot cannot duplicate loot',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  await game.act({keys:['Insert'],duration_ms:250});
  const retained=await game.observe();
  game.check('ordinary Nexus reconnect preserves the item and character',retained.world==='Nexus'&&retained.lifecycle.alive&&retained.session.character===picked.view.session.character&&retained.character.items[4]===item&&retained.character.inventory_revision===picked.view.character.inventory_revision);
}

