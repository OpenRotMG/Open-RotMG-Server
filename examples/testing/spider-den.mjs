import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x30e,level:20,seed:854,world:-2,position:'spawn',equipment:[0xb08,-1,0xb05,0xac5],backpack:[0x706,-1,-1,-1,-1,-1,-1,-1],stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const output=name=>join(process.env.QA_RESULTS_DIR,name),samples=[],protections=[];
  let protectionUntil=0;
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const token=(await readFile(output('native/state/control.token'),'utf8')).trim();
  async function save(name){const s=await game.inspect();samples.push({name,state:s});await writeFile(output('journey.json'),JSON.stringify(samples));return s;}
  async function protect(){
    const response=await fetch(report.origin+'/dev/testing',{method:'POST',signal:AbortSignal.timeout(10000),headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}})});
    const result=await response.json();assert(response.ok&&result.status==='succeeded',JSON.stringify(result));
    const applied=await game.observe();protectionUntil=applied.game_ms+60000;
    protections.push({game_ms:applied.game_ms,receipt:result});await writeFile(output('protection.json'),JSON.stringify(protections));
  }
  const entities=s=>s.diagnostic.entities;
  const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  async function walk(point,fire=false,tolerance=.75){
    for(let n=0;n<150;n++){
      const s=await game.inspect(),v=s.view,dx=point.x-v.position.x,dy=point.y-v.position.y,d=Math.hypot(dx,dy);
      if(d<tolerance)return;
      if(v.time_ms>=protectionUntil)await protect();
      const keys=Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']);
      const duration_ms=Math.max(100,Math.min(350,Math.round(Math.max(Math.abs(dx),Math.abs(dy))/(4+5.6*v.character.stats[4]/75)*1000)));
      const enemy=entities(s).filter(e=>e.kind>=0x21c&&e.kind<=0x220&&distance(e.pos,v.position)<7).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position))[0];
      await game.act({keys,duration_ms,...(fire&&enemy&&duration_ms>=150?{target:{entity:enemy.id,camera_angle:-Math.PI/4,fire:true}}:{})});
    }
    throw Error('Native walk did not reach '+JSON.stringify(point));
  }
  async function enter(from,name){
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
    for(let n=0;n<8&&(await game.observe()).session.world===from;n++)await game.act({keys:[],duration_ms:500});
    const s=await save(name);assert.notEqual(s.view.session.world,from);return s;
  }
  const initial=await save('nexus');
  await game.act({keys:['1'],duration_ms:500});
  const entry=await enter(initial.view.session.world,'key-entry');
  game.check('ordinary key consumption admits the same Wizard at Spider Den entrance',entry.view.map.name==='Spider Den'&&entry.view.session.character===initial.view.session.character&&entry.view.character.items[4]===-1&&distance(entry.view.position,{x:12.5,y:74.5})<1);
  await protect();
  await game.act({keys:[],duration_ms:700},{name:'spider-entrance',when:()=>true});
  await walk({x:12.5,y:55.5});
  const beforeTouch=await save('before-touch'),egg=entities(beforeTouch).find(e=>e.kind===0x21b&&distance(e.pos,{x:12.5,y:54.5})<.1);
  assert(egg,'First natural live sac was not preserved for touch test');
  const ids=new Set(entities(beforeTouch).map(e=>e.id));
  await walk(egg.pos,false,.4);
  const touched=await save('hatched-by-touch'),children=entities(touched).filter(e=>e.kind===0x21c&&!ids.has(e.id));
  game.check('touching natural live sac removes it and hatches two to seven spiders without firing',!entities(touched).some(e=>e.id===egg.id)&&children.length>=2&&children.length<=7&&touched.diagnostic.character.shots===beforeTouch.diagnostic.character.shots);
  await game.act({keys:[],duration_ms:500},{name:'spider-hatchlings',when:()=>true});
  await walk({x:8.5,y:54.5},true);
  await walk({x:6.2,y:54.5},true);
  const propBefore=await save('before-decorative-egg');
  await walk({x:8.8,y:54.5},true);
  const propAfter=await save('crossed-decorative-egg');
  game.check('ordinary walk crosses the natural decorative egg without collision',propBefore.view.position.x<7&&propAfter.view.position.x>8);
  await walk({x:12.5,y:54.5},true);
  await walk({x:28.5,y:54.5},true);
  const beforeShot=await save('before-shot-sac'),shotEgg=entities(beforeShot).find(e=>e.kind===0x21b&&distance(e.pos,{x:32.5,y:54.5})<.1);assert(shotEgg);
  const prior=new Set(entities(beforeShot).map(e=>e.id));
  await game.act({keys:[],duration_ms:80,target:{entity:shotEgg.id,camera_angle:-Math.PI/4,fire:true}});
  const shot=await save('hatched-by-shot'),survivors=entities(shot).filter(e=>e.kind===0x21c&&!prior.has(e.id));
  game.check('ordinary staff shot hatches another natural live sac with observed offspring',!entities(shot).some(e=>e.id===shotEgg.id)&&survivors.length>=1&&survivors.length<=7);
  for(const [x,y] of [[32.5,54.5],[32.5,34.5],[52.5,34.5],[72.5,34.5],[72.5,24.5]])await walk({x,y},true);
  const arena=await save('boss-room'),boss=entities(arena).find(e=>e.kind===0x936);assert(boss);
  game.check('entrance-to-boss route is traversable with ordinary populated rooms',arena.view.lifecycle.alive&&arena.diagnostic.character.kills>entry.diagnostic.character.kills&&arena.view.character.experience>entry.view.character.experience);
  // Wall-web rendering is not a Slowed source: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/SpiderWeb.as
  await walk({x:72.5,y:21.7},false,.4);
  await game.act({keys:[],duration_ms:2500},{name:'arachna-web',when:()=>true});
  const web=await save('web-and-attacks'),spokes=entities(web).filter(e=>e.kind>=0x937&&e.kind<=0x93f);
  game.check('Arachna establishes all nine web spokes and reaches vulnerable attack phase',spokes.length===9&&entities(web).find(e=>e.id===boss.id)?.stage===2&&!(entities(web).find(e=>e.id===boss.id).condition&(1<<24)));
  game.check('native web contact applies Slowed',web.view.character.conditions&(1<<3));
  for(let n=0;n<100;n++){
    const s=await game.inspect(),e=entities(s).find(e=>e.id===boss.id);if(!e)break;
    if(distance(s.view.position,e.pos)>6)await walk(e.pos,false,4);
    else await game.act({keys:[],duration_ms:300,target:{entity:e.id,camera_angle:-Math.PI/4,fire:true}});
  }
  await game.act({keys:[],duration_ms:500},{name:'arachna-defeated',when:()=>true});
  const defeated=await save('defeated');
  game.check('native staff defeats Arachna and awards one Spider Den completion',!entities(defeated).some(e=>e.id===boss.id)&&defeated.diagnostic.character.spider_dens===entry.diagnostic.character.spider_dens+1);
  const exit=entities(defeated).find(e=>e.kind===0x71d);assert(exit,'Boss did not publish Nexus exit');
  const bag=entities(defeated).find(e=>e.items.includes(0x707)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===defeated.view.session.account));assert(bag,'Eligible account did not receive personal Healing Ichor');
  await walk(bag.pos);
  await game.act({keys:[],duration_ms:500},{name:'arachna-rewards',when:()=>true});
  const beforePickup=await save('before-pickup');
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await save('picked');
  game.check('normal loot drag commits an Arachna reward exactly once',picked.view.character.items[4]>=0&&picked.view.character.inventory_revision===beforePickup.view.character.inventory_revision+1);
  await game.act({keys:[],duration_ms:1500});
  const stable=await save('stable-clear');
  game.check('clear credit and XP do not repeat after defeat',stable.diagnostic.character.spider_dens===defeated.diagnostic.character.spider_dens&&stable.view.character.experience===defeated.view.character.experience);
  await walk(exit.pos);
  const returned=await enter(entry.view.session.world,'boss-exit');
  game.check('boss exit returns the same living character and collected reward to opening Nexus',returned.view.map.name==='Nexus'&&returned.view.lifecycle.alive&&returned.view.session.character===initial.view.session.character&&JSON.stringify(returned.view.character.items)===JSON.stringify(picked.view.character.items)&&returned.diagnostic.character.spider_dens===stable.diagnostic.character.spider_dens);
  await game.act({keys:[],duration_ms:700},{name:'spider-return',when:()=>true});
}
