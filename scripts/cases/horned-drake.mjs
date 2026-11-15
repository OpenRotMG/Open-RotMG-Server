import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:806,world:1,equipment:[0xa9f,0xa2e,0xb05,0xac7],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
let parent,baby;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'drake-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-drakes.json'),JSON.stringify(map.entities.filter(e=>[0x945,0x946].includes(e.kind))));
  parent=map.entities.find(e=>e.kind===0x945&&[-2,-1,0].every(dx=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)].join(','))));assert(parent,'No naturally populated Horned Drake with a clear approach');
  baby=map.entities.find(e=>e.kind===0x946&&e.camp===parent.camp);assert(baby,'Natural Drake has no Baby');
  return {...spec,position:{x:parent.pos.x-2,y:parent.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const begin=await save('arrival'),hp=begin.view.character.hp;
  game.check('ordinary MidForest parent and Baby have selected HP',begin.diagnostic.entities.some(e=>e.id===parent.id&&e.hp===1270)&&begin.diagnostic.entities.some(e=>e.id===baby.id&&e.hp===700));
  await game.act({keys:[],duration_ms:1500},{name:'drake-arrival',when:()=>true});
  const exposed=await save('exposed');
  game.check('native enemy fire damages the Wizard',exposed.view.character.hp<hp);
  const path=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(path,JSON.stringify({id:'drake-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);
  const samples=[await game.inspect()];
  for(let n=0;n<16;n++){await game.act({keys:[],duration_ms:300},n===8?{name:'drake-attacks',when:()=>true}:undefined);samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify(samples));
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  game.check('natural family activates and moves',samples.some(s=>[parent,baby].every(e=>{const a=entity(s,e.id);return a&&a.activated!==null&&Math.hypot(a.pos.x-e.pos.x,a.pos.y-e.pos.y)>.1;})));
  for(const [e,lifetime] of [[parent,1500],[baby,1300]])game.check(e.kind+': selected speed/lifetime is observed',samples.some(s=>s.view.projectiles.some(p=>p.owner===e.id&&p.speed===65&&p.lifetime_ms===lifetime)));
  const shots=samples.slice(1).map((s,i)=>({at:s.diagnostic.time_ms,count:(entity(s,parent.id).bullet-entity(samples[i],parent.id).bullet+256)%256}));
  const total=shots.reduce((n,s)=>n+s.count,0),elapsed=samples.at(-1).diagnostic.time_ms-samples[0].diagnostic.time_ms;
  await writeFile(join(process.env.QA_RESULTS_DIR,'shot-counts.json'),JSON.stringify({total,elapsed,shots}));
  const before=await game.observe();
  for(let n=0;n<40;n++){
    const s=await game.inspect(),victim=[parent,baby].find(e=>entity(s,e.id));
    if(!victim)break;
    await game.act({keys:[],duration_ms:500,target:{entity:victim.id,camera_angle:-Math.PI/4,fire:true}});
  }
  await game.act({keys:[],duration_ms:300});
  const cleared=await save('family-defeated');
  game.check('native staff defeats both family members and awards XP',[parent,baby].every(e=>!entity(cleared,e.id))&&cleared.view.character.experience>before.character.experience);
  await game.act({keys:[],duration_ms:1000},{name:'drake-defeated',when:()=>true});
  const final=await save('final');
  game.check('family defeat does not duplicate XP',final.view.character.experience===cleared.view.character.experience);
  game.check('defeated camp has no replacement parent or Baby',!final.diagnostic.entities.some(e=>e.camp===parent.camp&&[0x945,0x946].includes(e.kind)));
  game.check('same prepared living Wizard completes the encounter',final.view.lifecycle.alive&&final.view.session.character===begin.view.session.character);
  game.check('observed time window has single/triple cadence, not repeated triple spam',total>=Math.floor(elapsed/2900)*3&&total<=Math.ceil(elapsed/2900)*5+5);
}
