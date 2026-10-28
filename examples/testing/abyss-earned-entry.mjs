import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:859,world:1,equipment:[0xb08,0xb24,0xb05,0xacd],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
let medusas;
export async function prepare(api){
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'abyss-entry-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),root=join(process.env.QA_RESULTS_DIR,'native'),report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  medusas=map.entities.filter(e=>e.kind===0x654&&[-1,0,1].every(dx=>[-1,0,1].every(dy=>!blocked.has([Math.floor(e.pos.x)-4+dx,Math.floor(e.pos.y)+dy].join(','))))).slice(0,40);
  assert(medusas.length,'No natural Medusa with a clear approach');return {...spec,position:{x:medusas[0].pos.x-4,y:medusas[0].pos.y}};
}
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,root=join(out,'native'),start=await game.observe();let serial=0,protectedUntil=0;
  async function command(command){const path=join(out,'request.json');await writeFile(path,JSON.stringify({id:'entry-'+serial++,command}));return JSON.parse(execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path],{encoding:'utf8'}));}
  async function protect(){const now=(await game.observe()).game_ms;if(now>=protectedUntil){await command({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});protectedUntil=now+60000;}}
  const attempts=[];let portal;
  for(const medusa of medusas){
    await protect();
    await command({op:'cheat',actor:game.actor,change:{kind:'teleport',position:{x:medusa.pos.x-4,y:medusa.pos.y}}});
    for(let n=0;n<20;n++){
      const s=await game.inspect(),enemy=s.diagnostic.entities.find(e=>e.id===medusa.id);if(!enemy)break;
      await game.act({keys:s.view.character.mp>=85?[' ']:[],duration_ms:500,target:{entity:medusa.id,camera_angle:-Math.PI/4,fire:true}});
    }
    await game.act({keys:[],duration_ms:500});
    const dead=await game.inspect();assert(!dead.diagnostic.entities.some(e=>e.id===medusa.id),'Natural Medusa not defeated');
    portal=dead.diagnostic.entities.find(e=>e.kind===0x71b);attempts.push({medusa:medusa.id,portal:portal?.id});
    await writeFile(join(out,'attempts.json'),JSON.stringify(attempts));if(portal)break;
  }
  assert(portal,'No Abyss portal in the bounded natural Medusa sample');
  const log=await readFile(join(root,'server.log'),'utf8');
  game.check('native Medusa defeat actually earns an Abyss portal',attempts.length>0&&log.includes('defeat earned a dungeon portal'));
  for(let n=0;n<60;n++){
    const v=await game.observe(),dx=portal.pos.x-v.position.x,dy=portal.pos.y-v.position.y;if(Math.hypot(dx,dy)<.6)break;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
    await game.act({keys:[Math.abs(x)>.2?(x>0?'d':'a'):null,Math.abs(y)>.2?(y>0?'s':'w'):null].filter(Boolean),duration_ms:80});
  }
  const at=await game.observe();game.check('native walking reaches the earned portal',Math.hypot(at.position.x-portal.pos.x,at.position.y-portal.pos.y)<1);
  await game.act({keys:[],duration_ms:800},{name:'abyss-earned-portal',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const entered=await game.inspect();
  game.check('earned portal enters Abyss with the same living character',entered.view.map.name==='Abyss of Demons'&&entered.view.session.character===start.session.character&&entered.view.lifecycle.alive&&entered.view.map.allow_teleport);
  await game.act({keys:[],duration_ms:900},{name:'abyss-earned-entry',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const returned=await game.inspect();
  game.check('Cowardice escape returns to the original Realm without clear credit',returned.view.session.world===start.session.world&&returned.view.lifecycle.alive&&returned.diagnostic.character.abysses===0);
  await writeFile(join(out,'earned-entry.json'),JSON.stringify({attempts,portal,entered,returned,scope:'Prepared level/gear/invincibility and explicit paused placement between natural Medusas. Native kills, natural drop rolls, walk/use and exit; not earned leveling, Realm traversal or a drop-rate estimate.'}));
}
