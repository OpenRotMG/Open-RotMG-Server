import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {earnChamberAdmission,PRIEST} from './xml-earned-journey.mjs';

// Second prepared boundary after the stronger continuous fighter died in the lower Castle (13f5e13f).
// Ordinary play ends at same-character Chamber admission. Oryx defeat, reward pickup and Cellar entry/return
// are out of earned scope and belong to the separate prepared/protected mechanics cases.
// Short input windows for dodge readback; all per-action and job deadlines are unchanged.
export const nativeReceiptLimit=448;
export const fixture={...PRIEST,level:20,class:0x31e,stats:[770,252,50,40,50,50,75,50],
  equipment:[0xa03,0xb22,0xafd,0xac8],backpack:[-1,0x707,0xc21,0xa23,0xa23,0xa23,0xa23,-1],world:'oryx-castle',position:{x:108.5,y:8.5}};
export async function run(game){
  const world=(await game.observe()).session.world;
  const priest={...PRIEST,level:20,seed:4103,world,position:{x:110.5,y:8.5},backpack:[0xc09,0xc21,0x707,0x707,0xa23,0xa23,0xa23,0xa23]};
  const healer={...priest,seed:4104,position:{x:106.5,y:9.5},backpack:[-1,...priest.backpack.slice(1)]};
  await writeFile(join(process.env.QA_RESULTS_DIR,'prepared-setup.json'),JSON.stringify({scope:'Prepared level-20 Knight and two Priests at the Castle Guardian approach; no runtime modifications',fixture,priest,healer}));
  const party=[await game.join(priest),await game.join(healer)];
  const initial=await game.inspect();
  game.check('prepared protagonist starts alive outside Guardian activation range',initial.view.map.name==="Oryx's Castle"&&initial.view.lifecycle.alive);
  await earnChamberAdmission(game,initial,party);
}
