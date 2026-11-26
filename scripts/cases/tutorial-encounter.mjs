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
    const {view,diagnostic}=await game.inspect(),target=diagnostic.entities.find(e=>e.id===entity.id);
    if(!target)return;
    const dx=target.pos.x-view.position.x,dy=target.pos.y-view.position.y,distance=Math.hypot(dx,dy);
    // After the long no-minions wait, the wandering God can be beyond Energy Staff's 8.55-tile lifetime (objects.xml:5889–5896).
    if(target.kind===0x6b1&&distance>6){await walk(game,{x:view.position.x+dx*(1-4/distance),y:view.position.y+dy*(1-4/distance)});continue;}
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
  // The recovered God only Follows/Wanders (monsters.xml:842ff; actors.xml:2176-2179), including after the legacy one-minute birth deadline.
  for(let i=0;i<7;i++)await game.act({keys:[],duration_ms:10000});
  const ready=await game.inspect();
  game.check('the living God does not spawn legacy minions',ready.diagnostic.entities.some(e=>e.id===god.id)&&!ready.diagnostic.entities.some(e=>e.kind===0x6b0||e.parent===god.id));
  game.check('chickens never shoot invented projectiles',ready.diagnostic.entities.filter(e=>e.kind>=0x6af&&e.kind<=0x6b2).every(e=>e.bullet===0));
  await defeat(game,god);
  await game.act({keys:[],duration_ms:1000},{name:'tutorial-victory',when:()=>true});
  const victory=await game.inspect();
  game.check('native staff fire defeats the God for six XP and clears its quest',!victory.diagnostic.entities.some(e=>e.id===god.id)&&victory.view.character.experience===ready.view.character.experience+6&&victory.view.quest!==god.id);
  game.check('God defeat grants no extra kill credit or drops',!victory.diagnostic.entities.some(e=>e.kind===0x6b0||e.items.some(i=>i>=0))&&victory.diagnostic.character.lifetime.kills===ready.diagnostic.character.lifetime.kills+1&&victory.diagnostic.character.lifetime.god_kills===ready.diagnostic.character.lifetime.god_kills);
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
  // Evil Hens award private Health Potion, not public Minor Health Potion (monsters.xml:856-866; actors.xml:2180-2182).
  const rewards=await game.inspect(),bags=rewards.diagnostic.entities.filter(e=>e.items.includes(0xa22));
  game.check('exit hens each drop exactly one private Health Potion',bags.length===3&&bags.every(b=>b.items.filter(i=>i>=0).length===1&&b.extra_stats.some(([stat,owner])=>stat===54&&owner===before.view.session.account)));
  const bag=bags[0];
  await walk(game,bag.pos);
  await game.act({keys:[],duration_ms:500},{name:'tutorial-loot',when:()=>true});
  await game.act({keys:[],duration_ms:750,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  game.check('ordinary loot drag takes the earned Health Potion',(await game.observe()).character.items[4]===0xa22);
  await walk(game,{x:12.5,y:4.5});
  await game.act({keys:[],duration_ms:1000},{name:'tutorial-exit',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:580},duration_ms:1500});
  const exit=await game.observe();
  game.check('ordinary Tutorial portal enters Kitchen',exit.world==='Kitchen');
  game.check('Tutorial combat, loot and exit preserve the same living character',exit.lifecycle.alive&&exit.session.character===before.view.session.character&&exit.character.items[0]===0xa97&&exit.character.items[4]===0xa22);
}
