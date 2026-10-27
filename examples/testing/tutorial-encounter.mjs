export const fixture={class:0x30e,level:1,seed:717,world:-1,position:{x:12.5,y:68.5},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
export async function run(game){
  const before=await game.inspect(),god=before.diagnostic.entities.find(e=>e.kind===0x6b1);
  game.check('authored chicken boss is visible',!!god);
  await game.act({keys:[],duration_ms:2500});
  const moved=await game.inspect(),current=moved.diagnostic.entities.find(e=>e.id===god.id);
  game.check('Evil Chicken God moves during ordinary native play',current&&Math.hypot(god.pos.x-current.pos.x,god.pos.y-current.pos.y)>0.1);
  for(let i=0;i<7;i++)await game.act({keys:[],duration_ms:10000});
  await game.act({keys:['s','d'],duration_ms:1000});
  await game.act({keys:[],duration_ms:1000},{name:'tutorial-minion',when:()=>true});
  const after=await game.inspect(),children=after.diagnostic.entities.filter(e=>e.kind===0x6b0);
  game.check('ordinary native play observes a delayed owned minion',children.length===1&&children[0].parent===god.id);
  game.check('chickens never shoot invented projectiles',after.diagnostic.entities.filter(e=>e.kind>=0x6af&&e.kind<=0x6b2).every(e=>e.bullet===0));
  game.check('same living tutorial character',after.view.lifecycle.alive&&after.view.session.character===before.view.session.character);
}
