import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x307,level:20,seed:871,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:[-1,-1,-1,-1,-1,-1,-1,-1],stats:[700,100,0,25,50,0,40,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  let serial=0;const observations=[];
  const save=()=>writeFile(join(root,'bows-observations.json'),JSON.stringify(observations,null,2));
  const cheat=async(actor,change)=>{
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'bows-'+(++serial),command:{op:'cheat',actor,change}}),signal:AbortSignal.timeout(10000)});
    const result=await response.json();assert(response.ok&&result.status!=='failed',JSON.stringify(result));
  };
  // Default 11.25-degree spacing is client logic, not a missing XML value: https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/com/company/assembleegameclient/objects/Player.as#L987-L1013
  for(const [item,count] of [[0xa1a,1],[0xa75,1],[0xa1b,1],[0xa76,1],[0xa77,1],[0xa1c,1],[0xa1d,2],[0xa3a,1],[0xa1e,3],[0xa8b,3],[0xa8c,3],[0xa8d,3],[0xb02,3],[0xb03,3],[0xb04,3],[0xc02,1],[0xc10,2],[0xc1b,1]]){
    await cheat(game.actor,{kind:'item',slot:4,item});
    await game.act({keys:[],duration_ms:200});
    await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:428},to:{x:634,y:384}}});
    const equipped=await game.observe();
    game.check(`native Archer equips ${item.toString(16)}`,equipped.character.items[0]===item);
    await game.act({keys:[],duration_ms:2200});
    const before=await game.inspect();
    await game.act({keys:[],mouse:{x:450,y:300},duration_ms:160},item===0xa1e?{name:'bows-golden',when:v=>v.verification.shots>before.diagnostic.character.shots}:undefined);
    const after=await game.inspect(),bullets=after.diagnostic.own_projectiles;
    observations.push({item,before,after});await save();
    game.check(`native ${item.toString(16)} admits one ${count}-arrow volley`,after.diagnostic.character.shots-before.diagnostic.character.shots===count);
    game.check(`native ${item.toString(16)} retains ${count} live arrows`,bullets.length===count);
    const angles=bullets.map(b=>b.angle).sort((a,b)=>a-b);
    game.check(`native ${item.toString(16)} spread`,angles.every((a,i)=>!i||Math.abs(a-angles[i-1]-Math.PI/16)<0.0001));
    game.check(`native ${item.toString(16)} is free and has no stat bonus`,after.view.character.mp===before.view.character.mp&&JSON.stringify(after.view.character.stats)===JSON.stringify(before.view.character.stats));
    await game.act({keys:[],duration_ms:700});
    game.check(`native ${item.toString(16)} arrows expire`,(await game.inspect()).diagnostic.own_projectiles.length===0);
    const rate=new Map([[0xa3a,1],[0xc02,0.33],[0xc10,1.25],[0xc1b,2.5]]).get(item);
    if(rate!==undefined){
      await game.act({keys:[],duration_ms:2200});
      const start=await game.observe();
      await game.act({keys:[],mouse:{x:450,y:300},duration_ms:3000});
      const end=await game.observe(),volleys=(end.verification.shots-start.verification.shots)/count;
      observations.push({cadence:item,start,end,volleys});await save();
      game.check(`native ${item.toString(16)} individual firing rate`,Number.isInteger(volleys)&&Math.abs(volleys-3000*0.0015*rate)<=1.5);
      await game.act({keys:[],duration_ms:700});
    }
  }
  const huntress=await game.join({...fixture,class:0x322,seed:872,world:'undead-lair',worldSeed:12351,position:{x:13.5,y:120.5},equipment:[0xa3a,-1,-1,-1],backpack:[0xa1d,0xa1e,0xc02,0xc10,0xc1b,-1,-1,-1]});
  await cheat(huntress.actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  for(const item of [0xa1a,0xa75,0xa1b,0xa76,0xa77,0xa1c,0xa1d,0xa3a,0xa1e,0xa8b,0xa8c,0xa8d,0xb02,0xb03,0xb04,0xc02,0xc10,0xc1b]){
    await cheat(huntress.actor,{kind:'item',slot:4,item});
    await huntress.act({keys:[],duration_ms:200});
    await huntress.act({keys:[],duration_ms:350,drag:{from:{x:634,y:428},to:{x:634,y:384}}});
    const equipped=await huntress.observe();game.check(`native Huntress equips ${item.toString(16)}`,equipped.character.items[0]===item);
    let before;
    for(let attempt=0;attempt<30;attempt++){
      before=await huntress.inspect();const boss=before.diagnostic.entities.find(e=>e.kind===0xd90);assert(boss,'living Septavius');
      const phase=(before.view.time_ms-boss.activated)%13000;
      if(phase>=3500&&phase<9500)break;
      before=null;await huntress.act({keys:[],duration_ms:500});
    }
    assert(before,'bounded Septavius vulnerability');
    if(!(before.view.character.conditions&(1<<24)))await cheat(huntress.actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
    const boss=before.diagnostic.entities.find(e=>e.kind===0xd90);
    await huntress.act({keys:[],target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true},duration_ms:2300});
    const after=await huntress.inspect();observations.push({combat:item,before,after});await save();
    game.check(`native ${item.toString(16)} damages living enemies`,after.diagnostic.entities.some(e=>{const old=before.diagnostic.entities.find(b=>b.id===e.id);return old&&e.hp<old.hp;})||before.diagnostic.entities.some(e=>!after.diagnostic.entities.some(b=>b.id===e.id)));
    game.check(`native ${item.toString(16)} preserves mana and item`,after.view.character.mp===before.view.character.mp&&after.view.character.items[0]===item&&after.view.lifecycle.alive);
  }
  const wizard=await game.join({...fixture,class:0x30e,seed:873,backpack:[0xc02,-1,-1,-1,-1,-1,-1,-1]});
  const before=await wizard.observe();
  await wizard.act({keys:[],duration_ms:350,drag:{from:{x:634,y:428},to:{x:634,y:384}}});
  const after=await wizard.observe();observations.push({refusal:{before,after}});await save();
  game.check('native Wizard cannot equip a bow atomically',after.character.items[0]===-1&&after.character.items[4]===0xc02&&after.character.inventory_revision===before.character.inventory_revision);
}
