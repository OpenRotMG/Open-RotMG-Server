import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x310,level:20,seed:872,world:'undead-lair',worldSeed:12351,position:'spawn',equipment:[0xa04,-1,-1,-1],backpack:[0xaf6,0xb3f,0xc04,0xc1d,0xc33,0xa00,0xa4b,-1],stats:[670,385,50,25,50,50,40,75]};
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect(),observations=[];
  const map=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});assert(map.ok);
  const layout=await map.json(),boss=layout.entities.find(e=>e.kind===0xd90);assert(boss);
  await writeFile(join(out,'map.json'),JSON.stringify(layout));
  await game.act({keys:[],duration_ms:400},{name:'wand-equipped',when:v=>v.character.items[0]===0xa04});
  for(const [slot,item,name,bonuses] of [[-1,0xa04,'fire-wand',[25,40,75]],[0,0xaf6,'recompense',[25,40,75]],[1,0xb3f,'crystal',[25,40,75]],[3,0xc1d,'abraham',[25,44,79]],[4,0xc33,'conducting',[26,40,76]],[6,0xa4b,'sprite',[25,40,75]],[2,0xc04,'bulwark',[25,40,75]]]){
    if(slot>=0)await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:428+44*Math.floor(slot/4)},to:{x:634,y:385}}});
    const before=await game.inspect();
    game.check(name+' native equip and bonuses',before.view.character.items[0]===item&&[3,6,7].every((s,i)=>before.view.character.stats[s]===bonuses[i]));
    await game.act({keys:[],mouse:{x:500,y:325},duration_ms:1800},{name,when:v=>v.game_ms>=before.diagnostic.time_ms+500});
    const after=await game.inspect();observations.push({name,before,after});
    game.check(name+' native fire admits shots without spending mana',after.diagnostic.character.shots>before.diagnostic.character.shots&&after.view.character.mp===before.view.character.mp);
    await game.act({keys:[],duration_ms:1000});
    if(item!==0xc04)game.check(name+' straight shots expire',(await game.inspect()).diagnostic.own_projectiles.length===0);
  }
  const moving=await game.inspect();
  game.check('Bulwark retains long-lived projectiles',moving.diagnostic.own_projectiles.length>0);
  const normal=observations.find(o=>o.name==='recompense'),slow=observations.find(o=>o.name==='bulwark');
  game.check('Bulwark fires more slowly than Recompense',slow.after.diagnostic.character.shots-slow.before.diagnostic.character.shots<(normal.after.diagnostic.character.shots-normal.before.diagnostic.character.shots)/2);
  for(const b of moving.diagnostic.own_projectiles){
    // The selected client's loop is not speed × lifetime: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/Projectile.as
    const t=(moving.diagnostic.time_ms-b.born)/11000*2*Math.PI;
    const x=Math.sin(t)*(b.id%2?1:-1),y=Math.sin(2*t)*(b.id%4<2?1:-1);
    const dx=(x*Math.cos(b.angle)-y*Math.sin(b.angle))*3,dy=(x*Math.sin(b.angle)+y*Math.cos(b.angle))*3;
    game.check('Bulwark observed parametric position '+b.id,Math.hypot(b.pos.x-b.start.x-dx,b.pos.y-b.start.y-dy)<0.001);
  }
  await game.act({keys:[],duration_ms:10000});
  await game.act({keys:[],duration_ms:2000});
  game.check('Bulwark expires after eleven seconds',(await game.inspect()).diagnostic.own_projectiles.length===0);
  const beforeDenied=await game.observe();
  await game.act({keys:[],duration_ms:350,drag:{from:{x:678,y:472},to:{x:634,y:385}}});
  const denied=await game.observe();
  game.check('Priest refuses incompatible sword atomically',JSON.stringify(denied.character.items)===JSON.stringify(beforeDenied.character.items)&&denied.character.inventory_revision===beforeDenied.character.inventory_revision);
  game.check('same living Priest',denied.lifecycle.alive&&denied.session.character===initial.view.session.character);
  await writeFile(join(out,'wand-observations.json'),JSON.stringify({observations,moving,beforeDenied,denied},null,2));
  const {worldSeed,...spec}=fixture;
  const sorcerer=await game.join({...spec,class:0x325,stats:[670,385,60,25,60,60,75,60],world:initial.view.session.world,position:{x:boss.pos.x-4,y:boss.pos.y}});
  const protection=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:sorcerer.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});
  const receipt=await protection.json();assert(protection.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  await writeFile(join(out,'sorcerer-protection.json'),JSON.stringify(receipt));
  await sorcerer.act({keys:[],duration_ms:2500});
  const sorcererBefore=await sorcerer.inspect();
  await sorcerer.act({keys:[],target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true},duration_ms:1000},{name:'sorcerer-damage',when:v=>v.game_ms>=sorcererBefore.diagnostic.time_ms+500});
  const sorcererAfter=await sorcerer.inspect();
  await writeFile(join(out,'sorcerer-damage.json'),JSON.stringify({preparedProtection:true,before:sorcererBefore,after:sorcererAfter}));
  game.check('Sorcerer can fire the same slot-eight wand',sorcererAfter.diagnostic.character.shots>sorcererBefore.diagnostic.character.shots);
  game.check('native wand shots damage an ordinary enemy',sorcererBefore.diagnostic.entities.some(e=>sorcererAfter.diagnostic.entities.find(q=>q.id===e.id)?.hp<e.hp));
  const wizard=await game.join({...spec,class:0x30e,stats:[670,385,75,25,50,75,40,60],world:initial.view.session.world,equipment:[-1,-1,-1,-1],backpack:[0xa04,-1,-1,-1,-1,-1,-1,-1]});
  const wizardBefore=await wizard.observe();
  await wizard.act({keys:[],duration_ms:350,drag:{from:{x:634,y:428},to:{x:634,y:385}}});
  const wizardAfter=await wizard.observe();
  game.check('Wizard refuses wand atomically',JSON.stringify(wizardAfter.character.items)===JSON.stringify(wizardBefore.character.items)&&wizardAfter.character.inventory_revision===wizardBefore.character.inventory_revision);
}
