import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x30e,level:20,seed:854,world:'spider-den',worldSeed:12351,position:{x:68.5,y:22.5},equipment:[-1,-1,0xb05,0xac5],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const samples=[];
  async function save(name){const state=await game.inspect();samples.push({name,state});await writeFile(join(process.env.QA_RESULTS_DIR,'cover.json'),JSON.stringify(samples));return state;}
  const start=await save('start');
  const pillar=start.diagnostic.entities.find(e=>e.kind===0x1e8&&e.pos.x===68.5&&e.pos.y===20.5);assert(pillar,'Natural southwest Spider Dirt Wall pillar missing');
  await game.act({keys:[],duration_ms:1500},{name:'spider-cover',when:()=>true});
  await game.act({keys:['d','w'],duration_ms:500});
  const blocked=await save('blocked');
  game.check('natural wall column blocks native movement before its occupied tile',blocked.view.position.y>=21&&blocked.view.position.y<22&&Math.abs(blocked.view.position.x-68.5)<.15);
  for(let n=0;n<8&&(await game.observe()).position.x>67.2;n++)await game.act({keys:['a','w'],duration_ms:200});
  for(let n=0;n<8&&(await game.observe()).position.y>20.4;n++)await game.act({keys:['d','w'],duration_ms:200});
  const around=await save('detour');
  game.check('ordinary living detour passes beside the column and decorative wall web',around.view.position.x<68&&around.view.position.y<20.5&&around.view.lifecycle.alive);
  game.check('collision test retains the same character without Paralyzed or Stasis',around.view.session.character===start.view.session.character&&samples.every(s=>!(s.state.view.character.conditions&((1<<13)|(1<<21)))));
  await game.act({keys:[],duration_ms:700},{name:'spider-cover-detour',when:()=>true});
}
