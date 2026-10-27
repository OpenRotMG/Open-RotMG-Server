import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x310,level:20,seed:766,world:-2,position:'spawn',equipment:[-1,0xa31,-1,-1],backpack:[0xb25,0xc09,0xc1e,-1,-1,-1,-1,-1],stats:[670,385,50,0,50,50,40,75]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const observations=[];
  async function vitals(actor,hp,mp){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change:{kind:'vitals',hp,mp}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  const initial=await game.observe();
  const ally=await game.join({...fixture,class:0x30e,world:initial.session.world,position:{x:initial.position.x+2,y:initial.position.y},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,50,0,50,50,40,60]});
  async function cast(name,amount,cost){
    await vitals(game.actor,100,300);await vitals(ally.actor,100,300);
    await game.act({keys:[],duration_ms:600});
    const before=await game.observe(),otherBefore=await ally.observe();
    await game.act({keys:[' '],duration_ms:250},{name,when:view=>view.character.mp<before.character.mp-cost/2});
    const after=await game.observe(),otherAfter=await ally.observe();
    observations.push({name,before,after,otherBefore,otherAfter});
    game.check(name+' spends one mana cost',after.character.mp>=before.character.mp-cost&&after.character.mp<before.character.mp-cost+10);
    game.check(name+' heals caster',after.character.hp>=before.character.hp+amount&&after.character.hp<before.character.hp+amount+10);
    game.check(name+' heals nearby native ally',otherAfter.character.hp>=otherBefore.character.hp+amount&&otherAfter.character.hp<otherBefore.character.hp+amount+10);
    return after;
  }
  await cast('t0-heal',100,60);
  async function equip(backpackSlot,item){
    await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(backpackSlot%4),y:428+44*Math.floor(backpackSlot/4)},to:{x:678,y:385}}});
    game.check('native inventory equipped '+item,(await game.observe()).character.items[1]===item);
  }
  await equip(0,0xb25);await cast('t6-heal',300,85);
  await equip(1,0xc09);await cast('purification-heal',250,100);
  await equip(2,0xc1e);const protectedView=await cast('holy-protection',250,100);
  game.check('Holy Protection gives caster Armored',Boolean(protectedView.character.conditions&(1<<25)));
  game.check('Holy Protection does not grant Invulnerable',!(protectedView.character.conditions&(1<<24)));
  game.check('Holy Protection does not armor the ally',!((await ally.observe()).character.conditions&(1<<25)));
  await game.act({keys:[],duration_ms:4200});
  game.check('Armored expires after four seconds',!((await game.observe()).character.conditions&(1<<25)));
  await vitals(game.actor,100,0);const poor=await game.observe();
  await game.act({keys:[' '],duration_ms:250});const denied=await game.observe();
  game.check('insufficient mana does not cast',denied.character.hp<poor.character.hp+10&&!(denied.character.conditions&(1<<25)));
  game.check('same living Priest and retained equipment',denied.lifecycle.alive&&denied.session.character===initial.session.character&&denied.character.items[1]===0xc1e);
  await writeFile(join(root,'tome-observations.json'),JSON.stringify({preparedVitals:true,observations,poor,denied},null,2));
}
