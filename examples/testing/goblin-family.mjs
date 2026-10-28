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
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'goblin-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);protectedUntil=now+60000;
  }
  const act=async(input,capture)=>{await protect();while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await game.act({keys:[],duration_ms:250});await protect();}await game.act(input,capture);};
  const begin=await save('arrival');
  assert(family(begin).some(e=>e.id===mage.id&&e.hp===280));
  await act({keys:['d'],duration_ms:600});
  const combat=[];
  for(let n=0;n<45;n++){
    const s=await game.inspect(),e=family(s).find(e=>e.id===mage.id);if(!e)break;
    const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,a=-Math.PI/4;
    const x=dx*Math.cos(a)+dy*Math.sin(a),y=-dx*Math.sin(a)+dy*Math.cos(a),length=Math.hypot(x,y);assert(length>0);
    // A short aim ray avoids the chat panel when the retreat projects near the canvas edge.
    await act({keys:[],duration_ms:250,mouse:{x:300+140*x/length,y:325+140*y/length}});
    await act({keys:[],duration_ms:650});combat.push(await game.inspect());
  }
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify(combat));
  await act({keys:[],duration_ms:1000});
  const dead=await save('mage-defeated'),remaining=family(dead);
  game.check('native wand defeats the natural Mage and awards XP',!family(dead).some(e=>e.id===mage.id)&&dead.view.character.experience>begin.view.character.experience);
  game.check('ordinary Mage defeat leaves surviving escorts',remaining.length>0);
  await act({keys:[],duration_ms:700},{name:'goblin-defeated',when:()=>true});
  for(let n=0;n<8;n++)await act({keys:[],duration_ms:2000});
  const end=await save('orphans');
  game.check('dead Mage cannot replenish escorts or transform',family(end).every(e=>e.kind!==0x614&&remaining.some(p=>p.id===e.id)));
  game.check('Mage XP is awarded once',end.view.character.experience===dead.view.character.experience);
  game.check('same prepared living character survives native encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
}
