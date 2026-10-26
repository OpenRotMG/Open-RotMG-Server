import {writeFile} from 'node:fs/promises';
export const fixture = {
  class:0x310, level:20, seed:12351, world:'snake-pit', worldSeed:0,
  position:world=>{
    if(world.width!==164||world.height!==140)throw Error('Snake Pit seed0 dimensions changed');
    return {x:143.5,y:33.5};
  },
  equipment:[0xaf6,-1,-1,-1], backpack:Array(8).fill(-1),
  stats:[670,385,50,25,50,50,40,75],
};
export async function run(game) {
  const initial=await game.observe();
  game.check('prepared living Priest in Snake Pit',initial.world==='Snake Pit'&&initial.lifecycle.alive);
  await game.act({keys:[],duration_ms:1500});
  await game.act({keys:[],duration_ms:1500},{name:'cover-gap',when:v=>v.lifecycle.alive});
  const inspection=await game.inspect();
  await writeFile(process.env.QA_RESULTS_DIR+'/cover-inspection.json',JSON.stringify(inspection,null,2));
  await game.act({keys:['s','d'],duration_ms:300});
  const moved=await game.observe();
  game.check('ordinary walking traverses the outer cover lane',moved.position.x>initial.position.x+.5&&Math.abs(moved.position.y-initial.position.y)<.2);
  game.check('same living character and world',moved.lifecycle.alive&&moved.session.character===initial.session.character&&moved.session.world===initial.session.world);
}
