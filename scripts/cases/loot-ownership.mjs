export const fixture={class:0x30e,level:1,seed:878,world:-1,position:{x:12.5,y:8.5},equipment:[0xa97,-1,-1,-1],backpack:[0xb0c,-1,-1,-1,-1,-1,-1,-1],stats:null};
export async function run(game){
  const other=await game.join({...fixture,backpack:Array(8).fill(-1)});
  const before=await game.observe();
  await game.act({keys:[],duration_ms:500},{name:'loot-start',when:()=>true});
  await game.act({keys:[],duration_ms:400,drag:{from:{x:634,y:428},to:{x:300,y:300}}});
  const dropped=await game.inspect(),privateBag=dropped.diagnostic.entities.find(e=>e.items?.includes(0xb0c));
  game.check('native Soulbound drop removes exactly one supplied tincture',dropped.view.character.items[4]===-1&&dropped.view.character.inventory_revision===before.character.inventory_revision+1&&!!privateBag);
  game.check('owner sees the private bag but another native account does not',dropped.view.objects.some(e=>e.id===privateBag.id)&&!(await other.inspect()).view.objects.some(e=>e.id===privateBag.id));
  const hen=dropped.diagnostic.entities.find(e=>e.kind===0x6b2);
  game.check('authored Tutorial hen exists',!!hen);
  for(let i=0;i<20&&(await game.inspect()).diagnostic.entities.some(e=>e.id===hen.id);i++)await game.act({keys:[],duration_ms:400,target:{entity:hen.id,camera_angle:-Math.PI/4,fire:true}});
  // Evil Hen's only row is SoulboundLoot prob=1 Health Potion (game/123.5.1/actors.xml:2182): private to its damager.
  // Rewards publish from the durable journal receipt, so the bag can appear a few ticks after the death.
  let killed=await game.inspect();
  for(let i=0;i<10&&!killed.diagnostic.entities.some(e=>e.items?.includes(0xa22));i++){await game.act({keys:[],duration_ms:300});killed=await game.inspect();}
  const henBag=killed.diagnostic.entities.find(e=>e.items?.includes(0xa22));
  game.check('ordinary staff combat earns one private hen reward',!killed.diagnostic.entities.some(e=>e.id===hen.id)&&!!henBag&&killed.view.objects.some(e=>e.id===henBag.id));
  game.check('noncontributor native account cannot see the soulbound reward',!(await other.inspect()).view.objects.some(e=>e.id===henBag.id));
  async function approach(client,goal){
    for(let i=0;i<40;i++){
      const v=await client.observe(),dx=goal.x-v.position.x,dy=goal.y-v.position.y;
      if(Math.hypot(dx,dy)<.5)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>.1)keys.push(x>0?'d':'a');
      if(Math.abs(y)>.1)keys.push(y>0?'s':'w');
      await client.act({keys,duration_ms:Math.min(350,Math.max(50,Math.round(Math.hypot(dx,dy)*130)))});
    }
    throw Error('Native approach did not reach loot');
  }
  await approach(game,henBag.pos);
  await game.act({keys:[],duration_ms:700},{name:'loot-private-reward',when:()=>true});
  const collecting=await game.observe();
  await game.act({keys:[],duration_ms:400,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await game.observe();
  game.check('eligible killer collects the earned potion once',picked.character.items[4]===0xa22&&picked.character.inventory_revision===collecting.character.inventory_revision+1);
  await game.act({keys:[],duration_ms:400,drag:{from:{x:634,y:528},to:{x:678,y:428}}});
  game.check('empty bag cannot duplicate its reward',(await game.observe()).character.inventory_revision===picked.character.inventory_revision);
  // Both comparators schedule removal, not public conversion: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/loot/Loots.cs#L142
  for(let i=0;i<7;i++){
    await game.act({keys:[],duration_ms:10000});
    game.check('private loot never becomes visible to the other native account at sample '+i,!(await other.inspect()).view.objects.some(e=>e.id===privateBag.id));
  }
  const expired=await game.inspect();
  game.check('nonempty private loot expires without becoming public',!expired.diagnostic.entities.some(e=>e.id===privateBag.id));
  game.check('collected reward bag is removed',!expired.diagnostic.entities.some(e=>e.id===henBag.id));
  game.check('expiry does not remove the collected potion or kill either character',expired.view.character.items[4]===0xa22&&expired.view.lifecycle.alive&&(await other.observe()).lifecycle.alive);
  await game.act({keys:[],duration_ms:800},{name:'loot-expired',when:()=>true});
}
