export const fixture = {
  class:0x310, level:20, seed:12351, world:'ocean-trench', position:'spawn',
  equipment:[0xaf6,0xb25,0xb05,0xac5], backpack:Array(8).fill(-1),
  stats:[670,385,50,25,50,50,40,75],
};
export async function run(game) {
  const start=await game.observe();
  const peer=await game.join({...fixture,world:start.session.world,position:{x:start.position.x+6,y:start.position.y}});
  const other=await peer.observe();
  game.check('two living players in the same Ocean Trench',start.world==='Ocean Trench'&&other.session.world===start.session.world&&other.lifecycle.alive);
  await game.act({keys:[],duration_ms:800});
  const shown=await game.observe();
  await game.act({keys:[],mouse:{x:650,y:520},duration_ms:1200},{name:'trench-menu',when:view=>view.game_ms>=shown.game_ms+400});
  const end=await game.observe();
  game.check('opening the player menu does not move either character',Math.hypot(end.position.x-other.position.x,end.position.y-other.position.y)>5);
  await game.act({keys:[],mouse:{x:550,y:400},duration_ms:1000},{name:'trench-teleported',when:view=>Math.hypot(view.position.x-other.position.x,view.position.y-other.position.y)<0.2});
  const arrived=await game.observe();
  game.check('native menu teleport reaches the other living player',arrived.session.world===start.session.world&&arrived.session.character===start.session.character&&arrived.lifecycle.alive&&Math.hypot(arrived.position.x-other.position.x,arrived.position.y-other.position.y)<0.2);
  await game.act({keys:['a','s'],duration_ms:200});
  const moved=await game.observe();
  game.check('native movement resumes after teleport acknowledgement',Math.hypot(moved.position.x-arrived.position.x,moved.position.y-arrived.position.y)>0.5&&moved.lifecycle.alive);
  await game.act({keys:[],mouse:{x:650,y:520},duration_ms:400});
  await game.act({keys:[],mouse:{x:550,y:400},duration_ms:400});
  const limited=await game.observe();
  game.check('native cooldown prevents immediate repeat teleport',limited.game_ms-arrived.game_ms<10000&&Math.hypot(limited.position.x-moved.position.x,limited.position.y-moved.position.y)<0.2);
}
