import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
export const fixture={class:0x30e,level:20,seed:834,world:'snake-pit',worldSeed:834,position:{x:43.5,y:45.5},equipment:[0xa9f,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export const run=game=>checkPythons(game,[0x225,0x227],fixture.position);
export async function checkPythons(game,kinds,position){
  const root=join(process.env.QA_RESULTS_DIR,'native'),config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()},initial=await game.inspect();
  const mapFor=async()=>{const r=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers});assert(r.ok);return r.json();};
  const map=await mapFor();await writeFile(join(process.env.QA_RESULTS_DIR,'natural-map.json'),JSON.stringify(map));
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const targets=kinds.map(k=>map.entities.find(p=>p.kind===k&&Math.abs(p.pos.x-position.x)<=4&&Math.abs(p.pos.y-position.y)<=4));
  assert(targets.every(Boolean),'Observed seeded room lacks the requested Python roles');
  const begin=await save('arrival'),samples=[],shots=new Map();
  game.check('generated room contains the selected 200-HP Pythons',targets.every(p=>begin.diagnostic.entities.some(e=>e.id===p.id&&e.hp===200)));
  const request=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(request,JSON.stringify({id:'python-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),request]);
  await game.act({keys:[],duration_ms:800},{name:'pythons-arrival',when:()=>true});
  for(let n=0;n<40;n++){
    await game.act({keys:[],duration_ms:150});const s=await game.inspect();samples.push(s);
    for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);
  }
  await game.act({keys:[],duration_ms:800},{name:'pythons-combat',when:()=>true});
  for(const p of targets){
    game.check(p.kind+': generated Python activates and moves',samples.some(s=>s.diagnostic.entities.some(e=>e.id===p.id&&e.activated!==null&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.2)));
    game.check(p.kind+': selected projectile appears during native combat',[...shots.values()].some(s=>s.owner===p.id&&s.speed===80&&s.lifetime_ms===(p.kind===0x225?2000:3000)));
  }
  // The dated guide supports three Fire shots, not exact cadence or loot odds: https://wikiwiki.jp/rmd/Fire%20Python
  if(kinds.includes(0x225))game.check('Fire Python emits six-degree three-shot fans',samples.some(s=>{
    const fan=s.view.projectiles.filter(p=>p.owner===targets.find(p=>p.kind===0x225).id);
    return fan.some(p=>[-1,1].every(sign=>fan.some(q=>Math.hypot(p.start.x-q.start.x,p.start.y-q.start.y)<.001&&Math.abs(q.angle-p.angle-sign*Math.PI/30)<.001)));
  }));
  for(const target of targets){
    const before=await game.inspect(),alive=before.diagnostic.entities.some(e=>e.id===target.id),xp=before.view.character.experience;
    for(let n=0;n<80;n++){
      const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===target.id);if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      if(Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===e.id))await game.act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:150});
      else await game.act({keys:[],duration_ms:250,target:{entity:e.id,camera_angle:-Math.PI/4,fire:true}});
    }
    const dead=await save('defeated-'+target.kind),world=await mapFor();
    game.check(target.kind+': native staff defeats observed Python',!world.entities.some(e=>e.id===target.id));
    game.check(target.kind+': living target earns XP; absent target follows earlier native collateral',alive?dead.view.character.experience>xp:targets.indexOf(target)>0&&before.view.character.experience>begin.view.character.experience);
  }
  await game.act({keys:[],duration_ms:3000},{name:'pythons-defeated',when:()=>true});
  const end=await save('end'),world=await mapFor();
  game.check('defeated Pythons remain absent',targets.every(p=>!world.entities.some(e=>e.id===p.id)));
  game.check('same prepared room Wizard survives',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({targets,samples,shots:[...shots.values()],rewardObjects:end.view.objects.filter(e=>[0x500,0x503,0x507,0x508,0x509].includes(e.kind))}));
}
