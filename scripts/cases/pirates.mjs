import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const spec={class:0x30e,level:2,seed:827,world:1,equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,25,25,30,20,0,0]};
let leader;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'pirate-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),root=join(process.env.QA_RESULTS_DIR,'native');
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  leader=map.entities.find(e=>e.kind===0x601&&Array.from({length:7},(_,i)=>!blocked.has([Math.floor(e.pos.x)-i,Math.floor(e.pos.y)].join(','))).every(Boolean));
  assert(leader,'No natural Piratess with a clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-pirates.json'),JSON.stringify(map.entities.filter(e=>e.camp===leader.camp)));
  return {...spec,position:{x:leader.pos.x-3,y:leader.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===leader.camp);
  const count=(s,kind)=>family(s).filter(e=>e.kind===kind).length;
  const begin=await save('arrival');
  game.check('natural ShoreSand camp begins with one 6HP Piratess and two 5HP Pirates',count(begin,0x601)===1&&count(begin,0x600)===2&&family(begin).every(e=>e.hp===(e.kind===0x600?5:6)));
  const samples=[];
  for(let n=0;n<20;n++){await game.act({keys:[],duration_ms:250,target:{entity:leader.id,camera_angle:-Math.PI/4,fire:false}});samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify(samples));
  const exposed=await save('exposed');
  game.check('native Wizard without immunity takes encounter damage',exposed.view.character.hp<begin.view.character.hp);
  for(const kind of [0x600,0x601]){
    const ids=new Set(family(begin).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': natural actors activate and move',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null&&Math.hypot(e.pos.x-family(begin).find(p=>p.id===e.id).pos.x,e.pos.y-family(begin).find(p=>p.id===e.id).pos.y)>.1)));
    game.check(kind+': selected 600ms speed40 Blade is observed',samples.some(s=>s.view.projectiles.some(p=>ids.has(p.owner)&&p.speed===40&&p.lifetime_ms===600)));
  }
  // This checks the bounded reconstruction, not the primary source's 60s density clock: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Reproduce.cs#L20-L24
  for(let n=0;n<10;n++)await game.act({keys:[],duration_ms:5000},n===7?{name:'pirates-population',when:()=>true}:undefined);
  const full=await save('grown');
  game.check('living camp reaches its reconstructed five Pirates and five Piratesses',count(full,0x600)===5&&count(full,0x601)===5);
  await game.act({keys:[],duration_ms:5000});
  const capped=await save('capped');
  game.check('full camp does not exceed its reserved capacity',family(capped).length===10&&family(capped).every(e=>family(full).some(p=>p.id===e.id)));
  const xp=capped.view.character.experience;
  for(let n=0;n<160&&family(await game.inspect()).length;n++){
    const target=family(await game.inspect())[0];
    await game.act({keys:[],duration_ms:250,target:{entity:target.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const dead=await save('defeated');
  game.check('native shooting defeats every member and earns XP',family(dead).length===0&&dead.view.character.experience>xp);
  await game.act({keys:[],duration_ms:6000});await game.act({keys:[],duration_ms:6000});
  const end=await save('finished');
  game.check('extinct camp does not reproduce or award XP twice',family(end).length===0&&end.view.character.experience===dead.view.character.experience);
  game.check('same living character without immunity completes the encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  await game.act({keys:[],duration_ms:1000},{name:'pirates-defeated',when:()=>true});
}
