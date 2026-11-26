import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const spec={class:0x30e,level:2,seed:827,world:1,equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,25,0,30,20,0,0]};
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
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-pirates.json'),JSON.stringify(map.entities.filter(e=>e.id===leader.id||e.parent===leader.id)));
  return {...spec,position:{x:leader.pos.x-1.5,y:leader.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  // XML Piratess MakeMinions Pirates (actors.xml:301) are owned by parent, not by the natural camp.
  const family=s=>s.diagnostic.entities.filter(e=>e.id===leader.id||e.parent===leader.id),owned=s=>family(s).filter(e=>e.parent===leader.id&&e.kind===0x600);
  const begin=await save('arrival');
  game.check('ordinary Realm places the selected root without a legacy camp',begin.diagnostic.entities.some(e=>e.id===leader.id&&e.camp==null&&e.hp===6));
  const samples=[];
  for(let n=0;n<20;n++){await game.act({keys:[],duration_ms:250,target:{entity:leader.id,camera_angle:-Math.PI/4,fire:false}});samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify(samples));
  const exposed=await save('exposed');
  game.check('native Wizard without immunity takes encounter damage',exposed.view.character.hp<begin.view.character.hp);
  const first=new Map();for(const s of samples)for(const e of family(s))if(!first.has(e.id))first.set(e.id,e);
  game.check('activated Piratess makes 5HP XML Pirates',[...first.values()].some(e=>e.kind===0x600)&&[...first.values()].every(e=>e.hp===(e.kind===0x600?5:6)));
  for(const kind of [0x600,0x601]){
    const ids=new Set([...first.values()].filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': actors activate and move from where they first appeared',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null&&Math.hypot(e.pos.x-first.get(e.id).pos.x,e.pos.y-first.get(e.id).pos.y)>.1)));
    game.check(kind+': selected 600ms speed40 Blade is observed',samples.some(s=>s.view.projectiles.some(p=>ids.has(p.owner)&&p.speed===40&&p.lifetime_ms===600)));
  }
  // The initial unprotected sample proves damage. Refill between later samples without shielding the Wizard.
  const root=join(process.env.QA_RESULTS_DIR,'native'),{origin}=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const token=(await readFile(join(root,'state/control.token'),'utf8')).trim();
  async function sustain(){
    const v=await game.observe();
    if(v.character.hp>=420)return;
    const r=await fetch(origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'vitals',hp:670,mp:v.character.mp}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await r.json();assert(r.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  for(let n=0;n<11;n++){
    await sustain();await game.act({keys:[],duration_ms:5000},n===10?{name:'pirates-population',when:()=>true}:undefined);
  }
  const full=await save('grown');
  game.check('living Piratess fills its six XML-owned Pirates',owned(full).length===6);
  for(let n=0;n<2;n++){await sustain();await game.act({keys:[],duration_ms:6000});}
  const capped=await save('capped');
  game.check('full Piratess does not exceed its six-Pirate cap',owned(capped).length===6&&owned(capped).every(e=>owned(full).some(p=>p.id===e.id)));
  const xp=capped.view.character.experience;
  for(let n=0;n<160&&family(await game.inspect()).length;n++){
    await sustain();
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
