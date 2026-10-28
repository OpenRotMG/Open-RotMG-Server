import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x30e,level:20,seed:855,world:'snake-pit',worldSeed:855,position:'spawn',equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(observer){
  const root=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  let serial=0;
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:'stheno-'+(++serial),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status!=='failed',JSON.stringify(s));return s;};
  const origin=await observer.observe();
  const map=async(player=origin.session.player)=>{const r=await fetch(config.origin+'/dev/map?player='+player,{headers,signal:AbortSignal.timeout(10000)});assert(r.ok);return r.json();};
  const layout=await map(),boss=layout.entities.find(e=>e.kind===0x917);assert(boss,'Generated Stheno missing');
  const {worldSeed,...spec}=fixture;
  const game=await observer.join({...spec,world:origin.session.world,position:{x:boss.pos.x-3,y:boss.pos.y}});
  const save=async name=>{const s=await game.inspect();await writeFile(join(root,name+'.json'),JSON.stringify(s));return s;};
  const initial=await save('stheno-arrival');
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  game.check('generated Stheno starts at selected 7500 HP',entity(initial,boss.id)?.hp===7500);
  await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});
  const samples=[],shots=new Map();let pending,error;
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
  async function sample(input,capture){
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>pending=null);},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;
  }
  for(let n=0;n<6;n++)await sample({keys:[],duration_ms:n===5?3500:6000},n===4?{name:'stheno-blind-ring',when:()=>true}:undefined);
  await writeFile(join(root,'stheno-phases.json'),JSON.stringify({boss,samples,shots:[...shots.values()]}));
  const stages=new Set(samples.map(s=>entity(s,boss.id)?.stage));
  game.check('Stheno runs silver swarm blind-ring and repeat stages',[0,1,2,3].every(n=>stages.has(n)));
  game.check('native client sees both selected Stheno projectile families',[...shots.values()].some(p=>p.owner===boss.id&&p.speed===70&&p.lifetime_ms===1500)&&[...shots.values()].some(p=>p.owner===boss.id&&p.speed===62&&p.lifetime_ms===2000));
  const active=samples.filter(s=>entity(s,boss.id)?.stage!==null);
  const radii=active.flatMap(s=>s.diagnostic.entities.filter(e=>e.kind===0x918&&e.parent===boss.id).map(e=>Math.hypot(e.pos.x-entity(s,boss.id).pos.x,e.pos.y-entity(s,boss.id).pos.y)));
  const pets=active.find(s=>s.diagnostic.entities.filter(e=>e.kind===0x918&&e.parent===boss.id).length===4)?.diagnostic.entities.filter(e=>e.kind===0x918&&e.parent===boss.id);
  game.check('four moving guardians stay on their reconstructed six-tile orbit',pets?.every(p=>active.some(s=>{const e=entity(s,p.id);return e&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>1;}))&&radii.length>20&&radii.every(r=>Math.abs(r-6)<.35));
  game.check('swarm appears in its phase and is removed for blind rings',samples.some(s=>entity(s,boss.id)?.stage===1&&s.diagnostic.entities.some(e=>e.kind===0x919&&e.parent===boss.id))&&samples.some(s=>entity(s,boss.id)?.stage===2&&!s.diagnostic.entities.some(e=>e.kind===0x919&&e.parent===boss.id)));
  const masks=samples.map(s=>s.view.objects.find(e=>e.id===boss.id)?.condition_mask).filter(Number.isInteger);
  game.check('native Stheno alternates invulnerable and vulnerable',masks.some(m=>m&(1<<24))&&masks.some(m=>!(m&(1<<24))));
  for(let n=0;n<24&&entity(await game.inspect(),boss.id);n++)await game.act({keys:[],duration_ms:1000,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  await game.act({keys:[],duration_ms:500});
  const defeated=await save('stheno-defeated'),cleared=await map(defeated.view.session.player);
  game.check('ordinary native staff defeats Stheno and grants XP',!cleared.entities.some(e=>e.id===boss.id)&&defeated.view.character.experience>initial.view.character.experience);
  const bag=cleared.entities.find(e=>e.items?.includes(0xa21)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===defeated.view.session.account));assert(bag,'No eligible private Speed reward');
  const toward=(a,b)=>{const dx=b.x-a.x,dy=b.y-a.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;return [Math.abs(x)>.15&&Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>.15&&Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean);};
  async function walk(point){
    for(let n=0;n<60;n++){const v=await game.observe(),d=Math.hypot(point.x-v.position.x,point.y-v.position.y);if(d<.4)return;await game.act({keys:toward(v.position,point),duration_ms:Math.max(50,Math.min(200,Math.round(d/(4+5.6*v.character.stats.speed/75)*1000)))});}
    throw Error('Native approach failed: '+JSON.stringify(point));
  }
  await walk(bag.pos);
  await game.act({keys:[],duration_ms:1000},{name:'stheno-reward',when:()=>true});
  const slot=bag.items.indexOf(0xa21),before=await game.observe();
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to:{x:634,y:428}}});
  const picked=await save('stheno-picked');
  game.check('native drag acquires earned Speed once',picked.view.character.items[4]===0xa21&&picked.view.character.inventory_revision===before.character.inventory_revision+1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to:{x:678,y:428}}});
  game.check('empty reward slot cannot duplicate Speed',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  const portal=cleared.entities.find(e=>[0x704,0x70e].includes(e.kind));assert(portal,'Boss return portal missing');
  await walk(portal.pos);
  await game.act({keys:[],duration_ms:800},{name:'stheno-exit',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  for(let n=0;n<8&&(await game.observe()).session.world===origin.session.world;n++)await game.act({keys:[],duration_ms:500});
  const returned=await save('stheno-returned');
  game.check('earned exit returns the same living character with its reward',returned.view.map.name==='Realm'&&returned.view.session.character===initial.view.session.character&&returned.view.lifecycle.alive&&returned.view.character.items[4]===0xa21);
  await game.act({keys:[],duration_ms:2500});
  await game.act({keys:['d'],duration_ms:300});
  const final=await save('stheno-final');
  game.check('return admission finishes and movement works',Math.hypot(final.view.position.x-returned.view.position.x,final.view.position.y-returned.view.position.y)>.1);
  game.check('Stheno XP is not awarded repeatedly',final.view.character.experience===defeated.view.character.experience);
  game.check('committed boss clear is counted once and survives the exit',defeated.view.character.snake_pits===initial.view.character.snake_pits+1&&picked.view.character.snake_pits===defeated.view.character.snake_pits&&returned.view.character.snake_pits===defeated.view.character.snake_pits&&final.view.character.snake_pits===returned.view.character.snake_pits);
  await writeFile(join(root,'stheno-outcome.json'),JSON.stringify({boss,bag,portal,initial,defeated,picked,returned,final,observer:await observer.inspect()}));
}
