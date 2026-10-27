import {writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const fixture={class:0x30e,level:5,seed:786,world:1,position:'spawn',equipment:[0xb08,0xa2e,0xb05,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function prepare(api){
  const scouts=[];
  for(const world of [1,2]){
    const info=await api({op:'world_info',world});
    const points=info.spawn_points.filter((_,i)=>i%Math.ceil(info.spawn_points.length/6)===0);
    for(const [x,y] of points){
      const spawned=await api({op:'spawn',spec:{...fixture,world,position:{x,y},guid:'swarm-scout-'+world+'-'+scouts.length,controller:'direct',profile:null}});
      const state=await api({op:'inspect',actor:spawned.result.actor});
      const target=state.view.objects.find(e=>e.id===state.view.quest);
      scouts.push({world,layout:info.realm_layout,position:{x,y},target});
      await writeFile(join(process.env.QA_RESULTS_DIR,'scouts.json'),JSON.stringify(scouts));
      if(target?.kind===0x955)return {...fixture,world,position:target.position};
    }
  }
  throw Error('No naturally populated Swarm quest found from twelve Realm entry scouts');
}
export async function run(game){
  let serial=0;
  const save=async name=>{const state=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(state));return state;};
  async function protect(){
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'swarm-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);
  }
  await protect();
  const initial=await save('initial'),target=initial.view.objects.find(e=>e.id===initial.view.quest);
  game.check('ordinary level-five Realm quest selects naturally populated Swarm',target?.kind===0x955);
  await game.act({keys:['d'],duration_ms:350});
  await game.act({keys:[],duration_ms:500},{name:'swarm-entry',when:()=>true});
  const first=await save('entry'),boss=first.diagnostic.entities.find(e=>e.id===target.id);
  game.check('natural Swarm activates without children or transformation',!!boss&&boss.activated!==null&&!first.diagnostic.entities.some(e=>e.parent===boss.id));
  const shots=new Map(),trace=[];let pending=null,error=null;
  function record(s){
    const b=s.diagnostic.entities.find(e=>e.id===boss.id);
    trace.push({time:s.view.time_ms,player:s.view.position,boss:b});
    for(const p of s.view.projectiles.filter(p=>p.owner===boss.id))if(!shots.has(p.id))shots.set(p.id,{...p,seen:s.view.time_ms});
  }
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},70);
  try{for(let n=0;n<3;n++){
    await game.act({keys:['s'],duration_ms:250});
    await game.act({keys:['w'],duration_ms:250});
    await game.act({keys:[],duration_ms:4100});
  }}
  finally{clearInterval(timer);if(pending)await pending;}
  if(error)throw error;
  const volleys=Map.groupBy([...shots.values()],p=>p.seen);
  await writeFile(join(process.env.QA_RESULTS_DIR,'cycle.json'),JSON.stringify({trace,shots:[...shots.values()],volleys:[...volleys]}));
  game.check('both circle and dart-away phases run',trace.some(t=>t.boss?.stage%2===0)&&trace.some(t=>t.boss?.stage%2===1));
  game.check('same Swarm moves during native play',trace.some(t=>t.boss&&Math.hypot(t.boss.pos.x-boss.pos.x,t.boss.pos.y-boss.pos.y)>1));
  game.check('native session receives directed and five-shot attacks',[...volleys.values()].some(v=>v.length===1)&&[...volleys.values()].some(v=>v.length===5));
  game.check('White Flames retain selected speed and lifetime',[...shots.values()].every(p=>p.speed===65&&p.lifetime_ms===1100));
  await game.act({keys:[],duration_ms:500},{name:'swarm-attacks',when:()=>true});
  const before=await game.observe();
  for(let n=0;n<24&&(await game.inspect()).diagnostic.entities.some(e=>e.id===boss.id);n++)await game.act({keys:[],duration_ms:500,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  const defeated=await save('defeated');
  game.check('normal native shooting defeats Swarm and awards XP once',!defeated.diagnostic.entities.some(e=>e.id===boss.id)&&defeated.view.character.experience>before.character.experience);
  game.check('defeat has no dependent successor',!defeated.diagnostic.entities.some(e=>e.parent===boss.id));
  game.check('same living character preserves supplied inventory',defeated.view.lifecycle.alive&&defeated.view.session.character===first.view.session.character&&JSON.stringify(defeated.view.character.items)===JSON.stringify(first.view.character.items));
  await game.act({keys:[],duration_ms:700},{name:'swarm-defeated',when:()=>true});
  const after=await game.observe();
  game.check('waiting cannot duplicate defeat XP',after.character.experience===defeated.view.character.experience);
}
