import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x30e,level:20,seed:859,world:1,position:'spawn',equipment:[0xb08,0xb24,0xb05,0xacd],backpack:[0x70a,-1,-1,-1,-1,-1,-1,-1],stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const before=await game.inspect();
  await game.act({keys:['1'],duration_ms:700});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const entered=await game.inspect();
  game.check('ordinary supplied key consumes once and enters Abyss',entered.view.map.name==='Abyss of Demons'&&entered.view.character.items[4]===-1&&entered.view.character.inventory_revision===before.view.character.inventory_revision+1);
  game.check('Abyss advertises same-world player teleport permission',entered.view.map.allow_teleport);
  game.check('entrance is not a dungeon completion',entered.diagnostic.character.abysses===before.diagnostic.character.abysses);
  await game.act({keys:[],duration_ms:900},{name:'abyss-key-entry',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const escaped=await game.inspect();
  game.check('Portal of Cowardice returns the same living character without clear credit',escaped.view.map.name==='Realm'&&escaped.view.session.world===before.view.session.world&&escaped.view.session.character===before.view.session.character&&escaped.view.lifecycle.alive&&escaped.diagnostic.character.abysses===before.diagnostic.character.abysses&&escaped.view.character.items[4]===-1);
  await writeFile(join(process.env.QA_RESULTS_DIR,'entry.json'),JSON.stringify({before,entered,escaped,scope:'Supplied key, ordinary native use and Cowardice escape; not earned Medusa portal acquisition.'}));
}
