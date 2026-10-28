import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
import {prepare as encounter} from './desert-werewolf.mjs';
let camp,leader;
export async function prepare(api){
  const spec=await encounter(api),members=JSON.parse(await readFile(join(process.env.QA_RESULTS_DIR,'natural-family.json'),'utf8'));
  leader=members.find(e=>e.kind===0x623);assert(leader);camp=leader.camp;return spec;
}
export async function run(game){
  const root=process.env.QA_RESULTS_DIR;
  const members=s=>s.diagnostic.entities.filter(e=>e.camp===camp);
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  const save=async name=>{const s=await game.inspect();await writeFile(join(root,name+'.json'),JSON.stringify(s));return s;};
  let protectedUntil=0,serial=0;
  async function act(input,capture){
    while(protectedUntil&&(await game.observe()).game_ms<protectedUntil&&(await game.observe()).game_ms+input.duration_ms+1500>=protectedUntil)await game.act({keys:[],duration_ms:100});
    const now=(await game.observe()).game_ms;
    if(now>=protectedUntil){
      const path=join(root,'protection.json');
      await writeFile(path,JSON.stringify({id:'warg-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native/native.json'),path]);protectedUntil=now+60000;
    }
    return game.act(input,capture);
  }
  await act({keys:[],duration_ms:1000},{name:'wargs-arrival',when:()=>true});
  const origin=(await game.observe()).position;
  await act({keys:['d'],duration_ms:100});const right=(await game.observe()).position;
  await act({keys:['s'],duration_ms:100});const down=(await game.observe()).position;
  const ux=right.x-origin.x,uy=right.y-origin.y,vx=down.x-right.x,vy=down.y-right.y,det=ux*vy-uy*vx;
  assert(Math.abs(det)>.05,'Native movement basis is degenerate');
  const camera=det>0?Math.atan2(uy,ux):Math.atan2(ux,-uy);
  async function shoot(id){
    for(let n=0;n<160;n++){
      const s=await game.inspect(),e=entity(s,id);if(!e)return;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx*vy-dy*vx)/det,y=(ux*dy-uy*dx)/det,approach=Math.hypot(dx,dy)>4;
      const keys=approach?[...(Math.abs(x)>Math.abs(y)*.5?[x>0?'d':'a']:[]),...(Math.abs(y)>Math.abs(x)*.5?[y>0?'s':'w']:[])]:[];
      await act({keys,duration_ms:250,target:{entity:id,camera_angle:camera,fire:!approach}});
    }
    assert.fail('Native shots failed to defeat '+id);
  }
  const arrival=await save('arrival'),demon=members(arrival).find(e=>e.kind===0x622);assert(demon);
  await shoot(demon.id);await act({keys:[],duration_ms:500});const defeated=await save('demon-defeated');
  game.check('native Demon defeat earns XP while Werewolf lives',!entity(defeated,demon.id)&&entity(defeated,leader.id)&&defeated.view.character.experience>=arrival.view.character.experience+4);
  for(let n=0;n<24;n++)await act({keys:[],duration_ms:500});const refill=await save('demon-replenished');
  game.check('living owner replenishes Demon Wargs within its cap',members(refill).filter(e=>e.kind===0x622).length===3&&members(refill).some(e=>e.kind===0x622&&!members(arrival).some(p=>p.id===e.id)));
  await shoot(leader.id);await act({keys:[],duration_ms:500},{name:'wargs-orphaned',when:()=>true});const orphaned=await save('orphaned');
  game.check('native owner defeat leaves both Warg types alive',!entity(orphaned,leader.id)&&[0x620,0x622].every(k=>members(orphaned).some(e=>e.kind===k)));
  // Kithio leaves children alive; its Protect reacquires any nearby Werewolf, unlike our owned-family reconstruction.
  // https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Protect.cs#L44-L97
  const samples=[];
  for(let n=0;n<26;n++){await act({keys:[],duration_ms:500});samples.push(await game.inspect());}
  await writeFile(join(root,'orphan-samples.json'),JSON.stringify(samples));
  for(const [kind,lifetime] of [[0x620,600],[0x622,2000]]){
    const initial=members(orphaned).filter(e=>e.kind===kind);
    game.check(kind+': same orphans persist, move and fire after owner death',initial.some(e=>samples.every(s=>entity(s,e.id))&&samples.some(s=>Math.hypot(entity(s,e.id).pos.x-e.pos.x,entity(s,e.id).pos.y-e.pos.y)>.1)&&samples.some(s=>s.view.projectiles.some(p=>p.owner===e.id&&p.speed===60&&p.lifetime_ms===lifetime))));
  }
  game.check('dead owner creates no replacement Wargs or repeat XP',samples.every(s=>members(s).every(e=>members(orphaned).some(p=>p.id===e.id))&&s.view.character.experience===orphaned.view.character.experience));
  const victim=members(samples.at(-1)).find(e=>e.kind===0x620);assert(victim);
  await shoot(victim.id);await act({keys:[],duration_ms:500});const dead=await save('orphan-defeated');
  game.check('native orphan Tawny defeat earns XP',!entity(dead,victim.id)&&dead.view.character.experience>=orphaned.view.character.experience+3);
  for(let n=0;n<24;n++)await act({keys:[],duration_ms:500});const end=await save('finished');
  game.check('defeated orphan stays absent without repeated XP',!entity(end,victim.id)&&members(end).every(e=>members(dead).some(p=>p.id===e.id))&&end.view.character.experience===dead.view.character.experience);
  game.check('same prepared character survives',end.view.lifecycle.alive&&end.view.session.character===arrival.view.session.character);
}
