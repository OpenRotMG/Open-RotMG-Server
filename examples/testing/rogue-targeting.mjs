import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {observeAction} from './rogue-cloaks.mjs';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x300,level:20,seed:766,world:'kitchen',position:{x:17.5,y:36.5},equipment:[0xa14,0xb27,0xa0d,-1],backpack:Array(8).fill(-1),stats:[720,252,50,25,75,75,40,50]};
export async function run(game){
  const before=await game.inspect(),boss=before.diagnostic.entities.find(e=>e.kind===0x926);
  game.check('natural Kitchen boss is present',!!boss);
  const visible=await game.join({...fixture,seed:767,world:before.view.session.world,position:{x:boss.pos.x-5,y:boss.pos.y},equipment:[-1,-1,0xa0d,-1]});
  await game.act({keys:[],duration_ms:1000});
  const awake=await game.inspect(),approaching=awake.diagnostic.entities.find(e=>e.id===boss.id);
  game.check('visible Rogue attracts the boss eastward',approaching.pos.x>boss.pos.x);
  const cloaked=await observeAction(game,'retargetActive',{keys:[' '],duration_ms:1200},game.inspect,state=>{
    const target=state.diagnostic.entities.find(e=>e.id===boss.id);
    if(!target)throw Error('Kitchen boss disappeared during cloak');
    return !!(state.view.character.conditions&(1<<12))&&target.pos.x<approaching.pos.x-0.5;
  });
  const after=await game.inspect(),following=cloaked.diagnostic.entities.find(e=>e.id===boss.id);
  await writeFile(join(root,'rogue-targeting.json'),JSON.stringify({before,awake,cloaked,after,visible:await visible.inspect()},null,2));
  game.check('nearer Rogue is invisible',!!(cloaked.view.character.conditions&(1<<12)));
  game.check('boss retargets westward from its last visible-Rogue position',following.pos.x<approaching.pos.x-0.5);
  game.check('both ordinary native participants survive',after.view.lifecycle.alive&&(await visible.observe()).lifecycle.alive);
  game.check('cloak keeps character and equipment while spending mana',after.view.session.character===before.view.session.character&&after.view.character.items.join()===before.view.character.items.join()&&after.view.character.inventory_revision===before.view.character.inventory_revision&&after.view.character.mp<before.view.character.mp);
  // Separate failed Nexus-load coverage: https://github.com/OpenRotMG/Open-RotMG-Server/issues/930#issuecomment-6450572904
}
