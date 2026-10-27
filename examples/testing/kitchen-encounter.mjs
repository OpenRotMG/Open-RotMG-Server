export const fixture={class:0x310,level:20,seed:717,world:'kitchen',position:{x:13.5,y:40.5},equipment:[0xa04,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,30,0,30,20,40,50]};
async function walk(game,goal){
  for(let i=0;i<100;i++){
    const view=await game.observe(),dx=goal.x-view.position.x,dy=goal.y-view.position.y,distance=Math.hypot(dx,dy);
    if(distance<0.4)return;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>0.12)keys.push(x>0?'d':'a');
    if(Math.abs(y)>0.12)keys.push(y>0?'s':'w');
    await game.act({keys,duration_ms:Math.min(300,Math.max(50,Math.round(distance*80)))});
  }
  throw Error('Native walk did not reach '+JSON.stringify(goal));
}
export async function run(game){
  const before=await game.inspect(),boss=before.diagnostic.entities.find(e=>e.kind===0x926);
  game.check('authored Bonegrind is visible',!!boss);
  await game.act({keys:[],duration_ms:6000});
  await game.act({keys:[],duration_ms:1000},{name:'kitchen-volley',when:()=>true});
  const active=await game.inspect(),chef=active.diagnostic.entities.find(e=>e.id===boss.id);
  game.check('Bonegrind attacks through ordinary native simulation',chef&&chef.bullet!==boss.bullet);
  for(let i=0;i<40;i++){
    const view=await game.inspect();
    if(!view.view.objects.some(e=>e.id===boss.id&&e.hp>0))break;
    await game.act({keys:[],duration_ms:500,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const victory=await game.inspect();
  game.check('native wand fire defeats Bonegrind',!victory.diagnostic.entities.some(e=>e.id===boss.id));
  await walk(game,{x:7.5,y:33.5});
  const door=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0x122&&Math.abs(e.pos.x-7.5)<0.1&&Math.abs(e.pos.y-30.5)<0.1);
  game.check('authored exit door is visible',!!door);
  await game.act({keys:[],duration_ms:2000,target:{entity:door.id,camera_angle:-Math.PI/4,fire:true}});
  game.check('native fire opens the Kitchen exit corridor',!(await game.inspect()).diagnostic.entities.some(e=>e.id===door.id));
  await walk(game,{x:7.5,y:27.5});
  await walk(game,{x:7.5,y:5.5});
  await game.act({keys:[],duration_ms:500},{name:'kitchen-exit',when:()=>true});
  const exit=await game.observe();
  await game.act({keys:[],mouse:{x:700,y:580},duration_ms:1500});
  const returned=await game.observe();
  game.check('ordinary Kitchen exit reaches a Realm',returned.world==='Realm');
  game.check('Kitchen combat and portal return preserve the living character and equipment',returned.lifecycle.alive&&returned.session.character===before.view.session.character&&JSON.stringify(returned.character.items)===JSON.stringify(exit.character.items));
}
