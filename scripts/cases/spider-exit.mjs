import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

const marker=world=>{const boss=world.encounter_markers.find(e=>e.kind===0x936);assert(boss,'Prepared arena requires Arachna');return boss.position;};
// Same south-centre approach as spider-den's natural corridor, outside the spoke ring at arrival.
export const fixture={class:0x30e,level:20,seed:1308,world:'spider-den',worldSeed:12351,
  position:world=>{const p=marker(world);return {x:p.x,y:p.y+9};},equipment:[0xb08,0xb24,0xb05,0xacd],backpack:Array(8).fill(0xa22),stats:[670,385,75,25,50,75,40,60]};
export const run=game=>arachnaCase(game,false);
export async function arachnaCase(game,webCheck){
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
  async function walk(point,tolerance=.75){
    for(let n=0;n<150;n++){
      const s=await game.inspect(),v=s.view;let dx=point.x-v.position.x,dy=point.y-v.position.y;
      if(Math.hypot(dx,dy)<tolerance)return;
      // Centre across the next tile's axis before pressing through a one-tile opening.
      if(Math.abs(dx)>Math.abs(dy)&&Math.floor(point.x)!==Math.floor(v.position.x)&&Math.abs(dy)>.24)dx=0;
      else if(Math.abs(dy)>Math.abs(dx)&&Math.floor(point.y)!==Math.floor(v.position.y)&&Math.abs(dx)>.24)dy=0;
      if(webCheck&&v.time_ms>=protectionUntil)await protect();
      const keys=Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']);
      const speed=v.character.conditions&8?4:4+5.6*v.character.stats[4]/75;
      const duration_ms=Math.max(50,Math.min(350,Math.round(Math.max(Math.abs(dx),Math.abs(dy))/speed*1000)));
      await game.act({keys,duration_ms});
    }
    throw Error('Native walk did not reach '+JSON.stringify(point));
  }
  async function enter(from,name){
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
    for(let n=0;n<8&&(await game.observe()).session.world===from;n++)await game.act({keys:[],duration_ms:500});
    const s=await save(name);assert.notEqual(s.view.session.world,from);return s;
  }
  const initial=await save('prepared-corridor'),entry=initial;
  const boss=entities(initial).find(e=>e.kind===0x936);assert(boss);
  game.check('prepared living Wizard arrives on the south-centre approach',initial.view.lifecycle.alive&&Math.abs(initial.view.position.x-boss.pos.x)<.5&&initial.view.position.y-boss.pos.y>8);
  if(webCheck)await protect();
  await game.act({keys:[],duration_ms:600},{name:'arachna-before',when:()=>true});
  await walk({x:boss.pos.x,y:boss.pos.y+5.2},.4);
  let active;
  for(let n=0;n<12;n++){
    active=await game.inspect();
    if(active.diagnostic.xml[boss.id]?.states.some(({name})=>name==='eat_flies')&&!(entities(active).find(e=>e.id===boss.id).condition&(1<<24)))break;
    await game.act({keys:[],duration_ms:350});
  }
  active=await save('vulnerable');
  game.check('Arachna reaches observed vulnerable eat_flies before native attack',active.diagnostic.xml[boss.id]?.states.some(({name})=>name==='eat_flies')&&!(entities(active).find(e=>e.id===boss.id).condition&(1<<24)));
  if(webCheck){
    // Wall-web rendering is not a Slowed source: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/SpiderWeb.as
    await game.act({keys:[],duration_ms:2500},{name:'arachna-web',when:()=>true});
    const web=await save('web-and-attacks'),spokes=entities(web).filter(e=>e.parent===boss.id&&e.kind>=0x937&&e.kind<=0x93f);
    game.check('Arachna establishes all nine web spokes and reaches vulnerable attack phase',spokes.length===9&&web.diagnostic.xml[boss.id]?.states.some(({name})=>name==='eat_flies')&&!(entities(web).find(e=>e.id===boss.id).condition&(1<<24)));
    // Recovered Spoke 9 fires east every 0.2 s; stand on its selected 3 s Slowed projectile ray.
    const spoke=spokes.find(e=>e.kind===0x93f);
    await walk({x:spoke.pos.x+2,y:spoke.pos.y},.18);
    await game.act({keys:[],duration_ms:1400});
    game.check('native web contact applies Slowed',(await save('web-thread-contact')).view.character.conditions&(1<<3));
  }
  for(let n=0;n<60;n++){
    const s=await game.inspect(),e=entities(s).find(e=>e.id===boss.id);if(!e)break;
    if(webCheck&&s.view.time_ms>=protectionUntil)await protect();
    if(!webCheck&&s.view.character.hp<360){
      const slot=s.view.character.items.findIndex((item,index)=>index>=4&&item===0xa22);
      if(slot>=4)await game.act({keys:[String(slot-3)],duration_ms:200});
    }
    const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,d=Math.hypot(dx,dy);
    if(d>5.5){
      const keys=Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']);
      await game.act({keys,duration_ms:220});
    }else await game.act({keys:!webCheck&&s.view.character.mp>=90?[' ']:[],duration_ms:400,target:{entity:e.id,camera_angle:-Math.PI/4,fire:true}});
  }
  await game.act({keys:[],duration_ms:500},{name:'arachna-defeated',when:()=>true});
  const defeated=await save('defeated');
  game.check((webCheck?'Protected':'Unprotected')+' native combat defeats Arachna and awards one Spider Den completion',!entities(defeated).some(e=>e.id===boss.id)&&defeated.diagnostic.character.spider_dens===entry.diagnostic.character.spider_dens+1);
  const exits=entities(defeated).filter(e=>e.kind===0x704||e.kind===0x71d);
  game.check('exactly one completion portal is a Realm exit',exits.length===1&&exits[0].kind===0x704);
  const [exit]=exits;
  let picked=defeated;
  if(webCheck){
  const bag=entities(defeated).find(e=>e.items.includes(0x707)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===defeated.view.session.account));assert(bag,'Eligible account did not receive personal Healing Ichor');
  await walk(bag.pos);
  await game.act({keys:[],duration_ms:500},{name:'arachna-rewards',when:()=>true});
  const beforePickup=await save('before-pickup');
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  picked=await save('picked');
  game.check('normal loot drag commits an Arachna reward exactly once',picked.view.character.items[4]>=0&&picked.view.character.inventory_revision===beforePickup.view.character.inventory_revision+1);
  }
  await game.act({keys:[],duration_ms:1500});
  const stable=await save('stable-clear');
  game.check('clear credit and XP do not repeat after defeat',stable.diagnostic.character.spider_dens===defeated.diagnostic.character.spider_dens&&stable.view.character.experience===defeated.view.character.experience);
  await walk(exit.pos);
  const returned=await enter(entry.view.session.world,'boss-exit');
  game.check('boss exit returns the same living character and collected reward to an open Realm',returned.view.map.name==='Realm'&&returned.view.lifecycle.alive&&returned.view.session.account===initial.view.session.account&&returned.view.session.character===initial.view.session.character&&JSON.stringify(returned.view.character.items)===JSON.stringify(picked.view.character.items)&&returned.diagnostic.character.spider_dens===stable.diagnostic.character.spider_dens);
  await game.act({keys:[],duration_ms:700},{name:'spider-return',when:()=>true});
}
