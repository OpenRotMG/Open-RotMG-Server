import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x30e,level:20,seed:1051,world:'spider-den',position:{x:12.5,y:71.5},equipment:[0xb08,-1,0xb05,0xac5],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const start=await game.inspect();
  const peer=await game.join({...fixture,world:start.view.session.world,position:{x:16.5,y:71.5}});
  const target=await peer.inspect();
  game.check('two living players begin apart in Spider Den',start.view.map.name==='Spider Den'&&target.view.session.world===start.view.session.world&&target.view.lifecycle.alive&&Math.hypot(start.view.position.x-target.view.position.x,start.view.position.y-target.view.position.y)>3);
  await game.act({keys:[],duration_ms:800});
  await game.act({keys:[],mouse:{x:650,y:520},duration_ms:800},{name:'spider-teleport-menu',when:()=>true});
  const before=await game.inspect();
  await writeFile(join(process.env.QA_RESULTS_DIR,'teleport-before.json'),JSON.stringify({start,target,before}));
  game.check('Spider Den MAPINFO enables native player teleport',before.view.map.allow_teleport);
  await game.act({keys:[],mouse:{x:550,y:400},duration_ms:1000},{name:'spider-teleport-arrival',when:view=>Math.hypot(view.position.x-target.view.position.x,view.position.y-target.view.position.y)<.2});
  const arrived=await game.inspect();
  game.check('native menu teleport reaches the same living Spider Den peer',arrived.view.session.world===start.view.session.world&&arrived.view.session.character===start.view.session.character&&arrived.view.lifecycle.alive&&Math.hypot(arrived.view.position.x-target.view.position.x,arrived.view.position.y-target.view.position.y)<.2);
  game.check('successful native teleport records lifetime use',arrived.diagnostic.character.lifetime.teleported);
  await game.act({keys:['a'],duration_ms:200});
  const moved=await game.observe();
  game.check('native movement resumes after GOTO acknowledgement',Math.hypot(moved.position.x-arrived.view.position.x,moved.position.y-arrived.view.position.y)>.3);
  await game.act({keys:[],mouse:{x:650,y:520},duration_ms:400});
  await game.act({keys:[],mouse:{x:550,y:400},duration_ms:400});
  const limited=await game.inspect();
  await writeFile(join(process.env.QA_RESULTS_DIR,'teleport-after.json'),JSON.stringify({arrived,moved,limited,peer:await peer.inspect()}));
  game.check('native cooldown refuses an immediate repeat without movement',limited.view.time_ms-arrived.view.time_ms<10000&&Math.hypot(limited.view.position.x-moved.position.x,limited.view.position.y-moved.position.y)<.2);
  game.check('teleport preserves living identity and inventory',limited.view.lifecycle.alive&&limited.view.session.character===start.view.session.character&&JSON.stringify(limited.view.character.items)===JSON.stringify(start.view.character.items));
}
