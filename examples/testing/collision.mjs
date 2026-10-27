import {readFile} from 'node:fs/promises';

export const fixture = {
  class:0x300, level:20, seed:12351, world:-2, position:{x:94.4,y:71.5},
  equipment:[-1,-1,-1,-1], backpack:Array(8).fill(-1), stats:[200,100,15,0,75,15,15,10],
};
export async function run(game) {
  const before=await game.observe();
  for (const keys of [['s','d'],['d'],['w'],['a'],['s'],['d','w'],['a','s']]) {
    const start=await game.observe();
    await game.act({keys,duration_ms:1000},{name:'collision-'+keys.join(''),when:()=>true});
    const end=await game.observe();
    game.check('native movement advances '+keys.join(''),Math.hypot(end.position.x-start.position.x,end.position.y-start.position.y)>1);
  }
  const after=await game.observe();
  game.check('same living character',after.lifecycle.alive&&after.session.character===before.session.character);
  const peer=await game.join({...fixture,seed:12352,position:{x:65.4,y:51.5}});
  await peer.act({keys:[],duration_ms:700});
  await peer.act({keys:['a'],duration_ms:450});
  const slid=await peer.observe();
  game.check('native player slides past FullOccupy wall corner',slid.position.x<64&&slid.position.y>51.5);
  const log=await readFile(process.env.QA_RESULTS_DIR+'/native/server.log','utf8');
  game.check('ordinary object collisions need no position correction',!log.includes('movement rejected; sent authoritative GOTO'));
}
