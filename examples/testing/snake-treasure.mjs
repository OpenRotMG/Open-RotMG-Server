import assert from 'node:assert/strict';

const GUARD=0xe26, BUTTON=0xe27, SPEED=0xa21;
const button={x:20.5,y:136.5}, center={x:20.5,y:148.5};
const bags=new Set([0x500,0x503,0x507,0x508,0x509]);
export const fixture={
  class:0x30e, level:20, seed:12351, world:'snake-pit', worldSeed:0,
  position(world){assert.equal(world.prepared_seed,0);return{x:button.x-0.9,y:button.y};},
  equipment:[0xb08,-1,0xb05,0xac5],
  backpack:[0xa22,0xa22,0xa22,0xa22,-1,-1,-1,-1],
  stats:[670,385,75,25,50,75,40,60],
};
const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
const guard=view=>view.nearby.find(e=>e.kind===GUARD);
async function healthy(game){
  const view=await game.observe();
  assert(view.lifecycle.alive,'Prepared character died');
  if(view.character.hp<350){
    const slot=view.character.items.findIndex((kind,i)=>i>=4&&i<8&&kind===0xa22);
    assert(slot>=4,'Finite carried health supply exhausted');
    await game.act({keys:[String(slot-3)],duration_ms:150});
  }
}
async function approach(game,target,radius){
  for(let i=0;i<40;i++){
    await healthy(game);
    const view=await game.observe(),dx=target.x-view.position.x,dy=target.y-view.position.y;
    if(distance(view.position,target)<=radius)return;
    const sx=(dx-dy)/Math.SQRT2,sy=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(sx)>0.08)keys.push(sx>0?'d':'a');
    if(Math.abs(sy)>0.08)keys.push(sy>0?'s':'w');
    await game.act({keys,duration_ms:Math.ceil(Math.max(80,Math.min(350,(Math.hypot(dx,dy)-radius)*110)))});
  }
  throw Error('Ordinary movement did not reach '+JSON.stringify(target));
}
export async function run(game){
  const before=await game.observe();
  assert(before.nearby.some(e=>e.kind===BUTTON),'Seeded treasure button is not visible');
  assert(!guard(before),'Treasure Guard was already present before pressing the button');
  await game.act({duration_ms:150},{name:'treasure-button-idle',when:v=>v.lifecycle.alive});
  await approach(game,button,0.3);
  await game.act({duration_ms:500});
  await approach(game,center,1);
  const awakened=await game.observe(),target=guard(awakened);
  assert(target&&target.hp===5500&&awakened.nearby.filter(e=>e.kind===GUARD).length===1,'Button did not summon exactly one full-health selected Guard');
  await game.act({duration_ms:250},{name:'treasure-guard-awake',when:v=>!!guard(v)});
  let wounded=false;
  for(let i=0;i<40;i++){
    await healthy(game);
    const view=await game.observe(),enemy=guard(view);
    if(!enemy){
      assert(view.verification.kills>before.verification.kills,'Guard left the bounded view without a verified kill');
      break;
    }
    wounded ||= enemy.hp<5500;
    const dx=enemy.position.x-view.position.x,dy=enemy.position.y-view.position.y;
    await game.act({shoot:true,mouse:{x:300+35.355*(dx-dy),y:325+35.355*(dx+dy)},duration_ms:400});
  }
  const defeated=await game.observe();
  assert(wounded,'Native weapon did not damage the Guard');
  assert(!guard(defeated)&&defeated.verification.kills===before.verification.kills+1,'Bounded normal combat did not defeat exactly one Guard');
  assert.equal(defeated.character.snake_pits,before.character.snake_pits,'Optional Guard granted a dungeon clear');
  const bag=defeated.nearby.find(e=>bags.has(e.kind));
  assert(bag,'Guard did not leave its nearby reward bag');
  await approach(game,bag.position,0.65);
  const inventory=await game.observe(),slot=inventory.character.items.findIndex((kind,i)=>i>=4&&kind===-1);
  assert(slot>=4,'No empty inventory slot for the guaranteed Speed potion');
  const dest={x:634+(slot%4)*44,y:slot<8?428:472};
  await game.act({mouse:{x:634,y:528},keys:['mouse1'],duration_ms:150});
  await game.act({mouse:dest,keys:['mouse1'],duration_ms:150});
  await game.act({mouse:dest,duration_ms:400});
  assert((await game.observe()).character.items.includes(SPEED),'Normal bag drag did not collect the guaranteed Speed potion');
  await game.act({duration_ms:200},{name:'treasure-speed-collected',when:v=>v.character.items.includes(SPEED)});
  await approach(game,button,0.3);
  await game.act({duration_ms:500});
  await approach(game,center,1);
  await game.act({duration_ms:500});
  const after=await game.observe();
  game.check('same living native character',after.lifecycle.alive&&after.session.character===before.session.character);
  game.check('optional Guard defeated with normal weapon',!guard(after)&&after.verification.kills>before.verification.kills);
  game.check('single-use button did not summon another Guard',!guard(after)&&after.verification.kills===defeated.verification.kills);
  game.check('Speed reward collected without dungeon-clear credit',after.character.items.includes(SPEED)&&after.character.snake_pits===before.character.snake_pits);
}
