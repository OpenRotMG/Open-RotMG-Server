import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:826,world:1,equipment:[0xb08,0xa2e,0xb05,0xac7],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
let kage;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'kage-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(process.env.QA_RESULTS_DIR,'native/result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(process.env.QA_RESULTS_DIR,'native/state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const graves=map.entities.filter(e=>e.kind===0x6db);
  assert.equal(graves.length,2,'Expected two normally generated graveyards');
  kage=graves.find(e=>[-3,-2,-1,0].every(dx=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)].join(','))));
  assert(kage,'No natural Kage grave with a clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-graves.json'),JSON.stringify(graves));
  return {...spec,position:{x:kage.pos.x-2.5,y:kage.pos.y}};
}
export async function run(game){
  const samples=[],shots=new Map();
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const entity=s=>s.diagnostic.entities.find(e=>e.id===kage.id);
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
  const step=async(input,capture)=>{
    let pending=null,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;
    const s=await game.inspect();record(s);return s;
  };
  const initial=await save('arrival');
  game.check('natural full-health grave is unrevealed',entity(initial).hp===7000&&entity(initial).stage===0);
  await step({keys:[],duration_ms:700},{name:'kage-grave',when:()=>true});
  game.check('proximity alone does not reveal Kage',entity(await game.inspect()).stage===0);
  // The March 2012 notes describe finding a graveyard, not spawning a visible boss: https://wikiwiki.jp/rmd/test/build122.2.1
  for(let n=0;n<16&&entity(await game.inspect()).stage===0;n++)await step({keys:[],duration_ms:100,target:{entity:kage.id,camera_angle:-Math.PI/4,fire:true}});
  const reveal=await save('reveal');
  game.check('ordinary staff damage reveals the same grave below ninety percent HP',entity(reveal).stage===1&&entity(reveal).hp<6300);
  const protectedHp=entity(reveal).hp;
  await step({keys:[],duration_ms:700,target:{entity:kage.id,camera_angle:-Math.PI/4,fire:true}},{name:'kage-reveal',when:()=>true});
  game.check('reveal animation rejects ordinary damage',entity(await game.inspect()).hp===protectedHp);
  for(let n=0;n<10&&(await game.inspect()).view.character.hp===initial.view.character.hp;n++)await step({keys:[],duration_ms:400});
  const exposed=await save('exposed');
  game.check('native enemy attacks damage the prepared Wizard',exposed.view.character.hp<initial.view.character.hp);
  const path=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(path,JSON.stringify({id:'kage-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);
  await step({keys:[],duration_ms:6000},{name:'kage-fight',when:()=>true});
  game.check('human and protected flying phases are both observed',[2,3].every(stage=>samples.some(s=>entity(s)?.stage===stage)));
  for(let n=0;n<20&&entity(await game.inspect()).stage!==3;n++)await step({keys:[],duration_ms:250});
  const flying=await game.inspect();assert.equal(entity(flying).stage,3);
  await step({keys:[],duration_ms:600,target:{entity:kage.id,camera_angle:-Math.PI/4,fire:true}});
  game.check('flying form rejects ordinary staff damage',entity(await game.inspect()).hp===entity(flying).hp);
  for(let n=0;n<100&&entity(await game.inspect());n++)await step({keys:[],duration_ms:400,target:{entity:kage.id,camera_angle:-Math.PI/4,fire:true}});
  const dead=await save('defeated');
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));
  game.check('native staff defeats Kage and awards XP',!entity(dead)&&dead.view.character.experience>initial.view.character.experience);
  game.check('human form wanders and flying form pursues', [2,3].every(stage=>{const ps=samples.filter(s=>entity(s)?.stage===stage).map(s=>entity(s).pos);return ps.some(p=>Math.hypot(p.x-ps[0].x,p.y-ps[0].y)>.2);}));
  game.check('client receives selected Specter Shots', [...shots.values()].some(p=>p.owner===kage.id&&p.speed===35&&p.lifetime_ms===2600));
  game.check('client receives selected flying Specter Spinners', [...shots.values()].some(p=>p.owner===kage.id&&p.speed===55&&p.lifetime_ms===2000));
  const mines=new Set(samples.flatMap(s=>s.diagnostic.entities.filter(e=>e.kind===0x6da).map(e=>e.id)));
  game.check('naturally fought Kage tosses Specter Mines',mines.size>0);
  game.check('mine bursts emit four spikes before expiring',samples.some(s=>s.diagnostic.entities.some(e=>mines.has(e.id)&&e.stage===1&&e.bullet===4))&&[...mines].every(id=>!dead.diagnostic.entities.some(e=>e.id===id)));
  const firstFlight=samples.find(s=>entity(s)?.stage===3).view.time_ms;
  game.check('native mine spikes apply Sick before flying attacks begin',samples.some(s=>s.view.time_ms<firstFlight&&(s.view.character.conditions&16)!==0));
  game.check('native projectiles apply Paralyzed, Sick and Confused',[8192,16,1024].every(mask=>samples.some(s=>(s.view.character.conditions&mask)!==0)));
  await step({keys:[],duration_ms:1000},{name:'kage-defeated',when:()=>true});
  const final=await save('finished');
  game.check('defeat awards XP once',final.view.character.experience===dead.view.character.experience);
  game.check('same supplied living Wizard completes the encounter',final.view.lifecycle.alive&&final.view.session.character===initial.view.session.character);
  const portals=final.view.objects.filter(e=>e.kind===0x739);
  await writeFile(join(process.env.QA_RESULTS_DIR,'portal-outcome.json'),JSON.stringify(portals));
  game.check('one defeat cannot create duplicate Manor entrances',portals.length<=1);
  if(portals.length){
    const portal=portals[0];
    await step({keys:[],duration_ms:1500});
    for(let n=0;n<30;n++){
      const s=await game.inspect(),dx=portal.position.x-s.view.position.x,dy=portal.position.y-s.view.position.y,distance=Math.hypot(dx,dy);
      if(distance<.7)break;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>.1)keys.push(x>0?'d':'a');
      if(Math.abs(y)>.1)keys.push(y>0?'s':'w');
      await step({keys,duration_ms:Math.min(250,Math.max(50,Math.round(distance*80)))});
    }
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
    for(let n=0;n<8&&(await game.observe()).session.world===initial.view.session.world;n++)await game.act({keys:[],duration_ms:500});
    const inside=await save('manor-arrival');
    game.check('awarded entrance admits the same living Wizard to Manor',inside.view.map.name==='Manor of the Immortals'&&inside.view.session.world!==initial.view.session.world&&inside.view.session.character===initial.view.session.character&&inside.view.lifecycle.alive);
    await game.act({keys:[],duration_ms:700},{name:'manor-arrival',when:()=>true});
  }
}
