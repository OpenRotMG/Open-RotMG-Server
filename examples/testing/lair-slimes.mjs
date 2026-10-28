import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';

export const fixture={class:0x31e,level:20,seed:858,world:'undead-lair',worldSeed:12351,position:'spawn',equipment:[0xa84,-1,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const root=join(process.env.QA_RESULTS_DIR,'native'),observations=[];
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8')),initial=await game.inspect();
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const black=map.entities.find(e=>e.kind===0xd9b&&[0,1,2].every(dx=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)].join(','))));assert(black,'No natural black slime approach');
  const {worldSeed,...knight}=fixture;
  game=await game.join({...knight,world:initial.view.session.world,position:{x:black.pos.x+2,y:black.pos.y}});
  async function split(parent,child,count,name){
    const before=await game.inspect(),ids=new Set(before.diagnostic.entities.map(e=>e.id));
    await game.act({keys:[],duration_ms:50,target:{entity:parent,camera_angle:-Math.PI/4,fire:true}});
    await game.act({keys:[],duration_ms:250});
    await game.act({keys:[],duration_ms:1000},{name,when:()=>true});
    const after=await game.inspect(),children=after.diagnostic.entities.filter(e=>!ids.has(e.id)&&e.kind===child);
    observations.push({name,parent,child,count,before,after});await writeFile(join(process.env.QA_RESULTS_DIR,'splits.json'),JSON.stringify(observations));
    game.check(name+': parent disappears and exact replacement generation survives',!after.diagnostic.entities.some(e=>e.id===parent)&&children.length===count);
    game.check(name+': intermediate death leaves no potion bag',!after.diagnostic.entities.some(e=>!ids.has(e.id)&&e.items?.some(item=>item!==-1)));
    game.check(name+': intermediate death retains period XP',after.view.character.experience>before.view.character.experience);
    return children;
  }
  const medium=await split(black.id,0xd9c,4,'black-medium');
  const position=(await game.observe()).position;
  medium.sort((a,b)=>Math.hypot(a.pos.x-position.x,a.pos.y-position.y)-Math.hypot(b.pos.x-position.x,b.pos.y-position.y));
  await split(medium[0].id,0xd9d,4,'black-little');
  const brown=map.entities.find(e=>e.kind===0xd9e&&[0,1,2].every(dx=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)].join(','))));assert(brown,'No natural brown slime approach');
  game=await game.join({...knight,world:initial.view.session.world,position:{x:brown.pos.x+2,y:brown.pos.y}});
  const protection=join(process.env.QA_RESULTS_DIR,'brown-protection.json');
  await writeFile(protection,JSON.stringify({id:'brown-lifecycle-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),protection]);
  await split(brown.id,0xd9f,6,'brown-little');
  game.check('native slime case ends with a living Knight',(await game.observe()).lifecycle.alive);
}
