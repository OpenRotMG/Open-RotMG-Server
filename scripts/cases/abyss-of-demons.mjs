import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const scene=process.env.ABYSS_SCENE??'clear',index=Number(process.env.ABYSS_IDOL??0),seed=scene==='idol'?19:0;
assert(['idol','phases','clear'].includes(scene)&&[0,1].includes(index));
let home;
export const fixture={class:0x30e,level:20,seed:859,world:'abyss-of-demons',worldSeed:seed,position(info){
  assert.equal(info.prepared_seed,String(seed));
  const sites=info.encounter_markers.filter(e=>e.kind===(scene==='idol'?0xe1d:0x90a)).sort((a,b)=>a.position.y-b.position.y||a.position.x-b.position.x);
  assert(sites[index]);home=sites[index].position;
  return {x:home.x+(scene==='idol'?0:2),y:home.y+(scene==='idol'?13:3)};
},equipment:[0xb08,0xb24,0xb05,0xacd],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,root=join(out,'native'),start=await game.inspect(),observed=[],distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);let serial=0;
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8')),token=(await readFile(join(root,'state/control.token'),'utf8')).trim();
  async function command(command){const path=join(out,'request.json');await writeFile(path,JSON.stringify({id:'scene-'+serial++,command}));return JSON.parse(execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path],{encoding:'utf8'}));}
  async function map(){const r=await fetch(report.origin+'/dev/map?player='+start.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(r.ok);return r.json();}
  async function save(name){const s=await game.inspect();observed.push({name,...s});await writeFile(join(out,'observed.json'),JSON.stringify(observed));return s;}
  async function walk(goal){
    for(let n=0;n<80;n++){
      const v=await game.observe(),dx=goal.x-v.position.x,dy=goal.y-v.position.y;if(Math.hypot(dx,dy)<.55)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      await game.act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:Math.max(50,Math.min(800,Math.round(Math.hypot(dx,dy)/7.5*1000)))});
    }
    throw Error('Native walking did not reach '+JSON.stringify(goal));
  }
  const generated=await map(),boss=generated.entities.find(e=>e.kind===0x90a),idols=generated.entities.filter(e=>e.kind===0xe1d),enemy=scene==='idol'?idols.find(e=>distance(e.pos,home)<.01):boss;
  assert(enemy&&boss);await writeFile(join(out,'generated-map.json'),JSON.stringify(generated));
  game.check('prepared seed resolves the natural selected full-health encounter',distance(enemy.pos,home)<.01&&enemy.hp===(scene==='idol'?25000:10000)&&idols.length===(scene==='idol'?2:0));
  await command({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});
  await game.act({keys:[],duration_ms:700},{name:scene+'-before',when:()=>true});
  let entrance=[];
  if(scene==='idol'){
    await walk({x:home.x,y:home.y+3});
    const dry=await save('dry');entrance=dry.view.tiles.filter(t=>Math.abs(t.x+.5-home.x)<=1&&t.y+.5>=home.y+7&&t.y+.5<=home.y+13);
    game.check('shrine entrance is dry before damage',entrance.length>=9&&entrance.every(t=>t.ground===0x3c));
    // The recovered Ready state places the overlay before damage; BlockExit tags guards, not lava (actors.xml:10908-10922).
    const idolState=dry.diagnostic.xml[enemy.id].states.map(s=>s.name);
    game.check('the natural Idol remains in Ready with a dry exit approach',idolState.includes('Ready')&&(await map()).entities.some(e=>e.id===enemy.id&&e.hp===25000));
  }
  if(scene==='phases'){
    // Malphas follows the seven named XML phases (actors.xml:2930-3017), not legacy numeric phase logs.
    const phases=['Missile_Fire','Pause1','Small_target','Size_matters','Pause2','Bring_on_the_flamers','Temporary_exhaustion'],seen=new Set();
    for(let n=0;n<65&&seen.size<phases.length;n++){
      await game.act({keys:[],duration_ms:1000},n===25?{name:'malphas-phases',when:()=>true}:undefined);
      const state=await save('phase-'+n);
      for(const phase of state.diagnostic.xml[boss.id].states)if(phases.includes(phase.name))seen.add(phase.name);
    }
    game.check('native encounter follows all seven Malphas phases',[...seen].join()===phases.join());
    game.check('native view receives boss attacks and changing sizes',observed.some(s=>s.view.projectiles.some(p=>p.owner===boss.id))&&observed.some(s=>s.diagnostic.entities.some(e=>e.id===boss.id&&e.size<100))&&observed.some(s=>s.diagnostic.entities.some(e=>e.id===boss.id&&e.size>100)));
    game.check('Protector Missile and Flamer births are observed',[0x908,0x909,0x90b].every(k=>observed.some(s=>s.diagnostic.entities.some(e=>e.kind===k&&e.parent===boss.id))));
    game.check('phase observation does not defeat or credit Malphas',(await map()).entities.some(e=>e.id===boss.id&&e.hp===10000)&&(await game.inspect()).diagnostic.character.abysses===0);
  }
  if(scene!=='phases'){
    for(let n=0;n<50;n++){
      const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===enemy.id);if(!e)break;
      if(distance(s.view.position,e.pos)>4)await walk(e.pos);
      const v=await game.observe();await command({op:'cheat',actor:game.actor,change:{kind:'vitals',hp:v.character.stats.max_hp,mp:v.character.stats.max_mp}});
      await game.act({keys:[' '],duration_ms:500,target:{entity:enemy.id,camera_angle:-Math.PI/4,fire:true}});
    }
    const defeated=await save('defeated'),all=await map();
    game.check('ordinary native attacks defeat the full-health encounter',!all.entities.some(e=>e.id===enemy.id)&&defeated.diagnostic.character.kills>start.diagnostic.character.kills);
    const bag=all.entities.find(e=>e.items.includes(0xa34)&&distance(e.pos,home)<12);assert(bag);
    // Recovered Malphas and Idol guarantee Vitality, not Defense (actors.xml:2921, 10970-10996).
    game.check('the defeated encounter grants private Vitality rather than Defense',bag.items.includes(0xa34)&&!bag.items.includes(0xa20)&&bag.extra_stats.some(([stat,owner])=>stat===54&&owner===start.view.session.account));
    await walk(bag.pos);await game.act({keys:[],duration_ms:700},{name:scene+'-loot',when:()=>true});
    for(const overflow of all.entities.filter(e=>e.id>bag.id&&e.items.some(i=>i>=0)&&distance(e.pos,bag.pos)<.01).sort((a,b)=>b.id-a.id)){
      for(const [slot,item] of overflow.items.entries())if(item>=0){
        const v=await game.observe(),empty=v.character.items.findIndex((i,n)=>n>=4&&i===-1);assert(empty>=4,'Overflow exceeds prepared inventory');
        await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:634+44*((empty-4)%4),y:428+44*Math.floor((empty-4)/4)}}});
        game.check('native overflow pickup commits the displayed item',(await game.observe()).character.items[empty]===item&&(await game.observe()).character.inventory_revision===v.character.inventory_revision+1);
      }
      await game.act({keys:[],duration_ms:500});game.check('empty overflow bag is removed',!(await map()).entities.some(e=>e.id===overflow.id));
    }
    const v=await game.observe(),slot=bag.items.indexOf(0xa34),empty=v.character.items.findIndex((i,n)=>n>=4&&i===-1);assert(empty>=4);
    await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:634+44*((empty-4)%4),y:428+44*Math.floor((empty-4)/4)}}});
    const picked=await save('picked');game.check('native Vitality pickup commits once',picked.view.character.items[empty]===0xa34&&picked.view.character.inventory_revision===v.character.inventory_revision+1);
    if(scene==='idol'){
      game.check('Idol defeat leaves the main encounter and other room untouched',picked.diagnostic.character.abysses===0&&all.entities.some(e=>e.id===boss.id&&e.hp===10000)&&all.entities.some(e=>e.kind===0xe1d&&e.id!==enemy.id&&e.hp===25000)&&!all.entities.some(e=>e.kind===0x704));
    }else{
      game.check('Malphas grants exactly one durable Abyss completion',picked.diagnostic.character.abysses===1);
      await game.act({keys:[],duration_ms:1000});game.check('completion does not repeat',(await game.inspect()).diagnostic.character.abysses===1);
      const portal=all.entities.find(e=>e.kind===0x704);assert(portal);await walk(portal.pos);
      await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
      const returned=await save('returned');game.check('completion exit retains the same living character and reward',returned.view.map.name==='Realm'&&returned.view.session.character===start.view.session.character&&returned.view.lifecycle.alive&&returned.diagnostic.character.abysses===1&&returned.view.character.items[empty]===0xa34);
    }
  }
  const end=await game.observe();game.check('same living character completes the bounded scene',end.lifecycle.alive&&end.session.character===start.view.session.character);
  await writeFile(join(out,'coverage.json'),JSON.stringify({scene,index,seed,idols:idols.length,scope:'Scout-derived seeded starting position, supplied level/gear, one timed Invincible and explicit HP/MP preparation during combat. Native input, actual encounter and rewards; not earned full traversal, ordinary-risk survival or historical acceptance.'}));
}
