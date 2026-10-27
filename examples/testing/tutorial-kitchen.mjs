export const fixture={class:0x30e,level:1,seed:717,world:-1,position:{x:12.5,y:68.5},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
export async function run(game){
  const before=await game.inspect(),god=before.diagnostic.entities.find(e=>e.kind===0x6b1);
  game.check('authored chicken boss is visible',!!god);
  await game.act({keys:[],duration_ms:2500},{name:'tutorial-encounter',when:()=>true});
  const moved=await game.inspect(),current=moved.diagnostic.entities.find(e=>e.id===god.id);
  game.check('Evil Chicken God moves during ordinary native play',current&&Math.hypot(god.pos.x-current.pos.x,god.pos.y-current.pos.y)>0.1);
  for(let i=0;i<7;i++)await game.act({keys:[],duration_ms:10000});
  await game.act({keys:[],duration_ms:1000},{name:'tutorial-minion',when:()=>true});
  const after=await game.inspect(),children=after.diagnostic.entities.filter(e=>e.kind===0x6b0);
  game.check('ordinary native play observes a delayed owned minion',children.length===1&&children[0].parent===god.id);
  game.check('chickens never shoot invented projectiles',after.diagnostic.entities.filter(e=>e.kind>=0x6af&&e.kind<=0x6b2).every(e=>e.bullet===0));
  game.check('same living tutorial character',after.view.lifecycle.alive&&after.view.session.character===before.view.session.character);

  const cook=await game.join({class:0x310,level:20,seed:717,world:'kitchen',position:{x:13.5,y:40.5},equipment:[0xa04,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,30,0,30,20,40,50]});
  const start=await cook.inspect(),boss=start.diagnostic.entities.find(e=>e.kind===0x926);
  cook.check('authored Bonegrind is visible',!!boss);
  await cook.act({keys:[],duration_ms:6000},{name:'kitchen-volley',when:()=>true});
  const active=await cook.inspect(),chef=active.diagnostic.entities.find(e=>e.id===boss.id);
  cook.check('Bonegrind attacks through ordinary native simulation',chef&&chef.bullet!==boss.bullet);
  for(let i=0;i<40;i++){
    const view=await cook.inspect();
    if(!view.view.objects.some(e=>e.id===boss.id&&e.hp>0))break;
    await cook.act({keys:[],duration_ms:500,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const victory=await cook.inspect();
  cook.check('native wand fire defeats Bonegrind',!victory.diagnostic.entities.some(e=>e.id===boss.id));
  cook.check('victory leaves the same living character',victory.view.lifecycle.alive&&victory.view.session.character===start.view.session.character);
  await cook.act({keys:['Insert'],duration_ms:1500},{name:'kitchen-return',when:view=>view.world==='Nexus'});
  const escaped=await cook.observe();
  cook.check('ordinary Kitchen escape returns the same living character to Nexus',escaped.world==='Nexus'&&escaped.lifecycle.alive&&escaped.session.character===start.view.session.character);

  const traveler=await game.join({...fixture,world:start.view.session.world,position:{x:7.5,y:5.5},equipment:[0xa97,-1,-1,-1]});
  const original=await traveler.observe();
  await traveler.act({keys:[],duration_ms:500},{name:'kitchen-exit',when:()=>true});
  await traveler.act({keys:[],mouse:{x:700,y:580},duration_ms:1500});
  const returned=await traveler.observe();
  traveler.check('ordinary Kitchen exit reaches a Realm',returned.world==='Realm');
  traveler.check('Kitchen portal return preserves the living character and equipment',returned.lifecycle.alive&&returned.session.character===original.session.character&&JSON.stringify(returned.character.items)===JSON.stringify(original.character.items));
}
