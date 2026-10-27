import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x300,level:20,seed:766,world:'kitchen',position:{x:17.5,y:36.5},equipment:[0xa14,0xb27,0xa0d,-1],backpack:Array(8).fill(-1),stats:[720,252,50,25,75,75,40,50]};
export async function run(game){
  const before=await game.inspect(),boss=before.diagnostic.entities.find(e=>e.kind===0x926);
  game.check('natural Kitchen boss is present',!!boss);
  const visible=await game.join({...fixture,seed:767,world:before.view.session.world,position:{x:boss.pos.x-5,y:boss.pos.y},equipment:[-1,-1,0xa0d,-1]});
  await game.act({keys:[],duration_ms:1000});
  const awake=await game.inspect(),approaching=awake.diagnostic.entities.find(e=>e.id===boss.id);
  game.check('visible Rogue attracts the boss eastward',approaching.pos.x>boss.pos.x);
  await game.act({keys:[' '],duration_ms:200});
  const cloaked=await game.inspect(),turn=cloaked.diagnostic.entities.find(e=>e.id===boss.id);
  game.check('nearer Rogue is invisible',!!(cloaked.view.character.conditions&(1<<12)));
  await game.act({keys:[],duration_ms:1200});
  const redirected=await game.inspect(),following=redirected.diagnostic.entities.find(e=>e.id===boss.id);
  await writeFile(join(root,'rogue-targeting.json'),JSON.stringify({before,awake,cloaked,redirected,visible:await visible.inspect()},null,2));
  game.check('boss retargets westward to the visible peer during cloak',following.pos.x<turn.pos.x-0.1);
  game.check('both ordinary native participants survive',redirected.view.lifecycle.alive&&(await visible.observe()).lifecycle.alive);
  await game.act({keys:['r'],duration_ms:1500});
  const nexus=await game.observe();
  game.check('escape reaches Nexus with the same living Rogue and cloak',nexus.world==='Nexus'&&nexus.session.world!==before.view.session.world&&nexus.session.character===before.view.session.character&&nexus.lifecycle.alive&&nexus.character.items[1]===0xb27);
}
