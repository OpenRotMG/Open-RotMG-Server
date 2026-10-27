export const fixture={class:0x30e,level:1,seed:717,world:-1,position:{x:12.5,y:68.5},equipment:[0xa97,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
async function walk(game,goal){
  for(let i=0;i<80;i++){
    const view=await game.observe(),dx=goal.x-view.position.x,dy=goal.y-view.position.y,distance=Math.hypot(dx,dy);
    if(distance<0.4)return;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>0.12)keys.push(x>0?'d':'a');
    if(Math.abs(y)>0.12)keys.push(y>0?'s':'w');
    await game.act({keys,duration_ms:Math.min(500,Math.max(50,Math.round(distance*120)))});
  }
  throw Error('Native walk did not reach '+JSON.stringify(goal));
}
async function defeat(game,entity){
  for(let i=0;i<20;i++){
    if(!(await game.inspect()).diagnostic.entities.some(e=>e.id===entity.id))return;
    await game.act({keys:[],duration_ms:500,target:{entity:entity.id,camera_angle:-Math.PI/4,fire:true}});
  }
  throw Error('Native fire did not defeat '+entity.id);
}
export async function run(game){
  const before=await game.inspect(),god=before.diagnostic.entities.find(e=>e.kind===0x6b1);
  game.check('authored chicken boss is visible',!!god);
  await game.act({keys:[],duration_ms:2500});
  const moved=await game.inspect(),current=moved.diagnostic.entities.find(e=>e.id===god.id);
  game.check('Evil Chicken God moves during ordinary native play',current&&Math.hypot(god.pos.x-current.pos.x,god.pos.y-current.pos.y)>0.1);
  game.check('level-one Tutorial quest targets the authored God',before.view.quest===god.id);
  game.check('minions are not spawned immediately',!moved.diagnostic.entities.some(e=>e.kind===0x6b0));
  // Real elapsed play preserves Kithio's minute-long Reproduce default: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Reproduce.cs
  for(let i=0;i<7;i++)await game.act({keys:[],duration_ms:10000});
  await walk(game,{x:12.5,y:64.5});
  await game.act({keys:[],duration_ms:1000},{name:'tutorial-minion',when:()=>true});
  const after=await game.inspect(),children=after.diagnostic.entities.filter(e=>e.kind===0x6b0);
  game.check('ordinary native play observes a delayed owned minion',children.length===1&&children[0].parent===god.id);
  game.check('chickens never shoot invented projectiles',after.diagnostic.entities.filter(e=>e.kind>=0x6af&&e.kind<=0x6b2).every(e=>e.bullet===0));
  const child=children[0],parent=after.diagnostic.entities.find(e=>e.id===god.id),distance=Math.hypot(child.pos.x-parent.pos.x,child.pos.y-parent.pos.y);
  await game.act({keys:[],duration_ms:2500});
  const protecting=await game.inspect(),minion=protecting.diagnostic.entities.find(e=>e.id===child.id),leader=protecting.diagnostic.entities.find(e=>e.id===god.id);
  game.check('owned minion remains near or approaches its parent',minion&&leader&&Math.hypot(minion.pos.x-leader.pos.x,minion.pos.y-leader.pos.y)<Math.max(1.5,distance));
  await defeat(game,child);
  const killed=await game.inspect();
  game.check('native staff fire defeats a minion for three XP',!killed.diagnostic.entities.some(e=>e.id===child.id)&&killed.view.character.experience===protecting.view.character.experience+3);
  for(let i=0;i<6;i++)await game.act({keys:[],duration_ms:10000});
  const refilled=await game.inspect();
  game.check('later reproduction replaces the dead owned minion',refilled.diagnostic.entities.some(e=>e.kind===0x6b0&&e.parent===god.id&&e.id!==child.id));
  await defeat(game,god);
  await game.act({keys:[],duration_ms:1000},{name:'tutorial-victory',when:()=>true});
  const victory=await game.inspect();
  game.check('native staff fire defeats the God for six XP and clears its quest',!victory.diagnostic.entities.some(e=>e.id===god.id)&&victory.view.character.experience===refilled.view.character.experience+6&&victory.view.quest!==god.id);
  game.check('God defeat retires owned minions without extra kill credit or drops',!victory.diagnostic.entities.some(e=>e.kind===0x6b0||e.kind===0x500)&&victory.diagnostic.character.lifetime.kills===refilled.diagnostic.character.lifetime.kills+1&&victory.diagnostic.character.lifetime.god_kills===refilled.diagnostic.character.lifetime.god_kills);
  await game.act({keys:[],duration_ms:1000});
  game.check('defeat rewards are not repeated by later ticks',(await game.inspect()).view.character.experience===victory.view.character.experience);
  for(const y of [43.5,30.5]){
    await walk(game,{x:12.5,y:y+4});
    const wall=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0x163&&e.pos.x===12.5&&e.pos.y===y);
    game.check('authored destructible wall blocks the exit route at '+y,!!wall);
    await defeat(game,wall);
    await walk(game,{x:12.5,y:y-3});
  }
  await walk(game,{x:12.5,y:8.5});
  const hens=(await game.inspect()).diagnostic.entities.filter(e=>e.kind===0x6b2);
  game.check('three authored exit hens remain available',hens.length===3);
  for(const hen of hens)await defeat(game,hen);
  const rewards=await game.inspect(),bags=rewards.diagnostic.entities.filter(e=>e.kind===0x500);
  game.check('exit hens each drop exactly one Minor Health Potion',bags.length===3&&bags.every(b=>b.items.filter(i=>i>=0).length===1&&b.items.includes(0xab0)));
  const bag=bags[0];
  await walk(game,bag.pos);
  await game.act({keys:[],duration_ms:500},{name:'tutorial-loot',when:()=>true});
  await game.act({keys:[],duration_ms:750,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  game.check('ordinary loot drag takes the earned Minor Health Potion',(await game.observe()).character.items[4]===0xab0);
  await walk(game,{x:12.5,y:4.5});
  await game.act({keys:[],duration_ms:1000},{name:'tutorial-exit',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:580},duration_ms:1500});
  const exit=await game.observe();
  game.check('ordinary Tutorial portal enters Kitchen',exit.world==='Kitchen');
  game.check('Tutorial combat, loot and exit preserve the same living character',exit.lifecycle.alive&&exit.session.character===before.view.session.character&&exit.character.items[0]===0xa97&&exit.character.items[4]===0xab0);
}
