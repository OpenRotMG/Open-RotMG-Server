import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x310,level:4,seed:813,world:1,equipment:[0xa04,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,25,0,50,25,40,60]};
let mage;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'goblin-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const root=join(process.env.QA_RESULTS_DIR,'native');
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  mage=map.entities.find(e=>e.kind===0x614&&Array.from({length:25},(_,i)=>!blocked.has([Math.floor(e.pos.x-10+i*.5),Math.floor(e.pos.y)].join(','))).every(Boolean));
  assert(mage,'No natural Goblin Mage with clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify(map.entities.filter(e=>e.camp===mage.camp)));
  return {...spec,position:{x:mage.pos.x-9.5,y:mage.pos.y}};
}
export async function run(game){
  let serial=0,protectedUntil=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===mage.camp&&[0x612,0x613,0x614].includes(e.kind));
  const births=new Map(),shots=new Map(),samples=[];
  const record=s=>{for(const e of family(s))births.set(e.id,e.kind);for(const p of s.view.projectiles)if(births.has(p.owner))shots.set(p.owner+':'+p.id,p);};
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'goblin-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);protectedUntil=now+60000;
  }
  const act=async(input,capture)=>{await protect();while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await game.act({keys:[],duration_ms:250});await protect();}await game.act(input,capture);record(await game.inspect());};
  const begin=await save('arrival');record(begin);
  game.check('ordinary LowForest camp starts with Mage and three of each escort',[[0x614,1],[0x612,3],[0x613,3]].every(([k,n])=>family(begin).filter(e=>e.kind===k).length===n));
  await game.act({keys:[],duration_ms:700},{name:'goblin-arrival',when:()=>true});
  await game.act({keys:['d'],duration_ms:600});
  const hp=(await game.observe()).character.hp;
  for(let n=0;n<20&&(await game.observe()).character.hp>=hp;n++)await game.act({keys:[],duration_ms:300});
  game.check('native player receives encounter damage',(await game.observe()).character.hp<hp);
  for(let n=0;n<35;n++){await act({keys:[],duration_ms:200});samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'movement-and-attacks.json'),JSON.stringify({samples,shots:[...shots.values()]}));
  game.check('Mage activates and moves',samples.some(s=>family(s).some(e=>e.id===mage.id&&e.activated!==null&&Math.hypot(e.pos.x-mage.pos.x,e.pos.y-mage.pos.y)>.2)));
  for(const [kind,speed,lifetime] of [[0x614,35,3000],[0x614,75,1600],[0x612,60,800],[0x613,60,600]])game.check(kind+': native selected projectile '+lifetime,[...shots.values()].some(p=>births.get(p.owner)===kind&&p.speed===speed&&p.lifetime_ms===lifetime));
  async function shoot(id){await act({keys:[],duration_ms:100,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});await act({keys:[],duration_ms:650});}
  async function defeat(id){
    const xp=(await game.observe()).character.experience;
    for(let n=0;n<35&&(await game.inspect()).diagnostic.entities.some(e=>e.id===id);n++)await shoot(id);
    const s=await game.inspect();game.check(id+': native shooting defeats enemy and earns XP',!s.diagnostic.entities.some(e=>e.id===id)&&s.view.character.experience>xp);
  }
  for(const kind of [0x612,0x613]){const e=family(await game.inspect()).find(e=>e.kind===kind);assert(e);await defeat(e.id);}
  for(let n=0;n<30;n++)await act({keys:[],duration_ms:2000});
  const grown=await save('birth-budget');
  for(const kind of [0x612,0x613])game.check(kind+': seven lifetime births, not seven live replacements',[...births.values()].filter(k=>k===kind).length===7&&family(grown).filter(e=>e.kind===kind).length===6);
  for(let n=0;n<15;n++){
    const e=family(await game.inspect()).find(e=>e.id===mage.id);assert(e);if(e.hp<182)break;await shoot(mage.id);
  }
  const wounded=await save('wounded');
  game.check('native wand crosses the 65-percent horde threshold',family(wounded).some(e=>e.id===mage.id&&e.hp>0&&e.hp<182&&e.stage===1));
  await act({keys:[],duration_ms:900},{name:'goblin-horde',when:()=>true});
  const retreat=[];for(let n=0;n<10;n++){await act({keys:[],duration_ms:300});retreat.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'retreat.json'),JSON.stringify(retreat));
  game.check('wounded Mage retreats toward six-tile spacing',retreat.some(s=>{const e=family(s).find(e=>e.id===mage.id);return e&&Math.hypot(e.pos.x-s.view.position.x,e.pos.y-s.view.position.y)>5.5;}));
  await defeat(mage.id);const dead=await save('mage-defeated'),remaining=family(dead);
  for(let n=0;n<8;n++)await act({keys:[],duration_ms:2000});
  const orphaned=await save('orphans');
  game.check('Mage death leaves escorts but cannot replenish or transform',remaining.length>0&&family(orphaned).every(e=>e.kind!==0x614&&remaining.some(p=>p.id===e.id)));
  game.check('Mage XP is awarded once',orphaned.view.character.experience===dead.view.character.experience);
  for(const kind of [0x612,0x613]){const e=family(await game.inspect()).find(e=>e.kind===kind);assert(e);await defeat(e.id);}
  const end=await save('finished');
  game.check('same prepared living character survives native encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  await act({keys:[],duration_ms:700},{name:'goblin-defeated',when:()=>true});
}
