export const fixture = {
  class:0x31e, level:20, seed:12351, world:-2, position:'spawn',
  equipment:[0xa00,-1,-1,-1], backpack:Array(8).fill(-1), stats:null,
};
export async function run(game) {
  const before=await game.observe();
  const cardinal=await game.act({keys:['d'],duration_ms:1200},{name:'movement',when:v=>Math.hypot(v.position.x-before.position.x,v.position.y-before.position.y)>2});
  const moved=await game.observe();
  game.check('ordinary native movement advances',Math.hypot(moved.position.x-before.position.x,moved.position.y-before.position.y)>2);
  const diagonalInput=await game.act({keys:['a','w'],duration_ms:1200});
  const diagonal=await game.observe();
  game.check('diagonal native movement advances',Math.hypot(diagonal.position.x-moved.position.x,diagonal.position.y-moved.position.y)>1);
  // The client uses atan2, not summed axis speeds: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/Player.as#L538-L543
  const speed=input=>input.effects.distance/(input.steps[0].game_ms-input.steps[0].before_ms);
  const ratio=speed(diagonalInput)/speed(cardinal);
  game.check('diagonal speed is normalized, not sqrt(2) faster',ratio>0.85&&ratio<1.15);
  await game.act({keys:[],duration_ms:1000});
  const idle=await game.observe();
  game.check('idle remains stationary',Math.hypot(idle.position.x-diagonal.position.x,idle.position.y-diagonal.position.y)<0.05);
  game.check('same living character',idle.lifecycle.alive&&idle.session.character===before.session.character);
  await game.act({keys:['o'],duration_ms:250});
  await game.act({keys:['d'],duration_ms:500},{name:'movement-options',when:()=>true});
  const options=await game.observe();
  game.check('options intercept movement without applying Paused',Math.hypot(options.position.x-idle.position.x,options.position.y-idle.position.y)<0.05&&(options.character.conditions&(1<<21))===0);
  await game.act({keys:[],mouse:{x:60,y:226},duration_ms:150});
  await game.act({keys:['o'],duration_ms:250});
  await game.act({keys:['e'],duration_ms:500});
  const turned=await game.observe();
  game.check('camera rotation alone does not move the player',Math.hypot(turned.position.x-idle.position.x,turned.position.y-idle.position.y)<0.05);
  await game.act({keys:['d'],duration_ms:800},{name:'rotated-movement',when:v=>Math.hypot(v.position.x-turned.position.x,v.position.y-turned.position.y)>1});
  const rotated=await game.observe();
  const dx=moved.position.x-before.position.x,dy=moved.position.y-before.position.y;
  const rx=rotated.position.x-turned.position.x,ry=rotated.position.y-turned.position.y;
  game.check('same key follows the rotated camera, not the old world axis',Math.hypot(rx,ry)>1&&Math.abs((dx*rx+dy*ry)/(Math.hypot(dx,dy)*Math.hypot(rx,ry)))<0.5);
  const peer=await game.join({...fixture,class:0x324,seed:12352,position:idle.position,equipment:[0xa14,0xae3,-1,-1]});
  const peerBefore=await peer.observe();
  await peer.act({keys:['d'],duration_ms:500});
  const peerMoved=await peer.observe();
  const watching=await game.observe();
  game.check('second native player moves',Math.hypot(peerMoved.position.x-peerBefore.position.x,peerMoved.position.y-peerBefore.position.y)>1);
  game.check('first client sees the second player',watching.nearby.some(o=>o.id===peerMoved.session.player));
  game.check('second client sees the first player',peerMoved.nearby.some(o=>o.id===watching.session.player));
  await peer.act({keys:[' '],mouse:{x:500,y:300},duration_ms:250});
  const teleported=await peer.observe();
  game.check('ordinary prism teleports and spends mana',Math.hypot(teleported.position.x-peerMoved.position.x,teleported.position.y-peerMoved.position.y)>3&&teleported.character.mp<peerMoved.character.mp);
  await peer.act({keys:['a'],duration_ms:600},{name:'teleport-movement',when:v=>v.lifecycle.alive&&Math.hypot(v.position.x-teleported.position.x,v.position.y-teleported.position.y)>1});
  const resumed=await peer.observe();
  game.check('movement resumes after GOTO acknowledgement',Math.hypot(resumed.position.x-teleported.position.x,resumed.position.y-teleported.position.y)>1);
  game.check('teleported character remains connected and alive',resumed.lifecycle.alive&&resumed.session.character===peerBefore.session.character);
}
