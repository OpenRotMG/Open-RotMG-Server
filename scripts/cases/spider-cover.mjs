import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

// The arena's south-west pillar stands 10 tiles west and 11-12 south of Arachna; start two tiles below it.
const arachna=w=>{const b=w.encounter_markers.find(e=>e.kind===0x936);assert(b,'Arachna missing');return b.position;};
export const fixture={class:0x30e,level:20,seed:854,world:'spider-den',worldSeed:12351,position:w=>{const b=arachna(w);return{x:b.x-10,y:b.y+14};},equipment:[-1,-1,0xb05,0xac5],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const samples=[];
  async function save(name){const state=await game.inspect();samples.push({name,state});await writeFile(join(process.env.QA_RESULTS_DIR,'cover.json'),JSON.stringify(samples));return state;}
  const start=await save('start');
  const me=start.view.position,pillar=start.diagnostic.entities.find(e=>e.kind===0x1e8&&Math.abs(e.pos.x-me.x)<.6&&me.y-e.pos.y>1&&me.y-e.pos.y<3);assert(pillar,'Natural southwest Spider Dirt Wall pillar missing');
  await game.act({keys:[],duration_ms:1500},{name:'spider-cover',when:()=>true});
  await game.act({keys:['d','w'],duration_ms:500});
  const blocked=await save('blocked');
  game.check('natural wall column blocks native movement before its occupied tile',blocked.view.position.y>=pillar.pos.y+.5&&blocked.view.position.y<pillar.pos.y+1.5&&Math.abs(blocked.view.position.x-pillar.pos.x)<.15);
  for(let n=0;n<8&&(await game.observe()).position.x>pillar.pos.x-1.3;n++)await game.act({keys:['a','w'],duration_ms:200});
  for(let n=0;n<8&&(await game.observe()).position.y>pillar.pos.y-1.1;n++)await game.act({keys:['d','w'],duration_ms:200});
  const around=await save('detour');
  game.check('ordinary living detour passes beside the column and decorative wall web',around.view.position.x<pillar.pos.x-.5&&around.view.position.y<pillar.pos.y-1&&around.view.lifecycle.alive);
  game.check('collision test retains the same character without Paralyzed or Stasis',around.view.session.character===start.view.session.character&&samples.every(s=>!(s.state.view.character.conditions&((1<<13)|(1<<21)))));
  await game.act({keys:[],duration_ms:700},{name:'spider-cover-detour',when:()=>true});
}
