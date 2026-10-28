import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x323,level:20,seed:867,world:'wine-cellar',position:'spawn',equipment:[0xb08,0xae4,-1,-1],backpack:[0x722,-1,-1,-1,-1,-1,-1,-1],stats:[670,385,60,25,60,50,40,75]};
export async function run(game){
  await game.act({keys:[],duration_ms:2200});
  const before=await game.inspect();
  game.check('prepared Cellar guest is alive',before.view.map.name==='Wine Cellar'&&before.view.lifecycle.alive);
  await game.act({keys:['Insert'],duration_ms:1700});
  const after=await game.inspect();
  await writeFile(join(process.env.QA_RESULTS_DIR,'cellar-escape.json'),JSON.stringify({before,after}));
  game.check('selected Insert binding reaches Nexus',after.view.map.name==='Nexus'&&after.view.lifecycle.alive);
  game.check('escape preserves the same character and inventory',after.view.session.character===before.view.session.character&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items)&&after.view.character.experience===before.view.character.experience);
}
