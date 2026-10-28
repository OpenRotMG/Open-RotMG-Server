import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';
export const fixture={class:0x30e,level:20,seed:879,world:'pirate-cave',position:'spawn',equipment:[-1,0xa2e,-1,0xa25],backpack:[0xa2a,0xa3c,0xa23,0xa23,0xa23,0xa23,0xc0c,0xa4a],stats:[670,385,75,25,50,75,40,0]};
export async function run(game){
  const root=join(process.env.QA_RESULTS_DIR,'native'),records=[];
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const record=async label=>{const state=await game.inspect();records.push({label,...state});await writeFile(join(root,'inventory-observations.json'),JSON.stringify(records));return state.view;};
  const drag=(from,to)=>game.act({keys:[],duration_ms:350,drag:{from,to}});
  const double=async mouse=>{await game.act([{actor:game.actor,input:{keys:[],duration_ms:40,mouse}},{actor:game.actor,input:{keys:[],duration_ms:40,mouse}},{actor:game.actor,input:{keys:[],duration_ms:450}}]);};
  await game.act({keys:['d'],duration_ms:500});
  const initial=await record('initial');
  await game.act({keys:[],duration_ms:700},{name:'inventory-before',when:()=>true});
  game.check('prepared inventory has four equipment and eight occupied carried slots',initial.character.items.length===12&&initial.character.items.slice(4).every(x=>x!==-1));
  await double({x:634,y:428});
  const equipped=await record('ring-swap');
  game.check('contextual carried ring swap commits once and replaces rather than stacks defense',equipped.character.items[3]===0xa2a&&equipped.character.items[4]===0xa25&&equipped.character.stats[3]===30&&equipped.character.inventory_revision===initial.character.inventory_revision+1);
  await double({x:678,y:428});
  const refused=await record('incompatible');
  game.check('incompatible sword contextual click is atomic',JSON.stringify(refused.character.items)===JSON.stringify(equipped.character.items)&&refused.character.inventory_revision===equipped.character.inventory_revision&&JSON.stringify(refused.character.stats)===JSON.stringify(equipped.character.stats));
  await drag({x:766,y:384},{x:300,y:325});
  const dropped=await record('ring-ground');
  game.check('native equipped drop reverses defense with a full carried inventory',dropped.character.items[3]===-1&&dropped.character.stats[3]===25&&dropped.character.items.slice(4).every(x=>x!==-1)&&dropped.character.inventory_revision===refused.character.inventory_revision+1);
  // Pre-124 contextual clicks equip directly when carried slots are full: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/ui/_E_6.as
  await double({x:634,y:528});
  const groundEquipped=await record('ground-ring-equipped');
  game.check('full-inventory ground contextual click equips directly exactly once',groundEquipped.character.items[3]===0xa2a&&groundEquipped.character.stats[3]===30&&JSON.stringify(groundEquipped.character.items.slice(4))===JSON.stringify(dropped.character.items.slice(4))&&groundEquipped.character.inventory_revision===dropped.character.inventory_revision+1);
  await game.act({keys:[' '],duration_ms:200});
  const spent=await record('spent');
  game.check('ordinary aimed spell spends mana',spent.character.mp<groundEquipped.character.mp);
  await drag({x:722,y:428},{x:300,y:325});
  const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'item',slot:6,item:0xa23}}}),signal:AbortSignal.timeout(10000)});
  const prepared=await response.json();assert(response.ok&&prepared.status==='succeeded',JSON.stringify(prepared));records.push({explicitFullInventoryPreparation:prepared});
  await game.act({keys:[],duration_ms:300});
  const full=await record('ground-potion-full');
  game.check('explicit refill prepares full inventory and a real dropped potion',full.character.items.slice(4).every(x=>x!==-1)&&(await game.inspect()).diagnostic.entities.some(e=>e.items?.includes(0xa23)));
  await double({x:634,y:528});
  const used=await record('ground-potion-used');
  game.check('full-inventory ground contextual use restores mana without consuming a carried potion',used.character.mp>full.character.mp&&JSON.stringify(used.character.items)===JSON.stringify(full.character.items)&&used.character.inventory_revision===full.character.inventory_revision+1);
  await double({x:634,y:528});
  game.check('repeating emptied ground slot cannot duplicate consumption',(await game.observe()).character.inventory_revision===used.character.inventory_revision);
  await game.act({keys:[' '],duration_ms:200});
  const beforeHotkey=await game.observe();
  await game.act({keys:['3'],duration_ms:300});
  const hotkey=await record('hotkey');
  game.check('ordinary carried hotkey consumes exactly one potion and restores mana',hotkey.character.items[6]===-1&&hotkey.character.mp>beforeHotkey.character.mp&&hotkey.character.inventory_revision===beforeHotkey.character.inventory_revision+1);
  await drag({x:634,y:428},{x:300,y:325});
  const beforePickup=await record('free-slot-pickup-before');
  await double({x:634,y:528});
  const picked=await record('free-slot-pickup');
  game.check('with space contextual ground click picks up rather than equips',picked.character.items[4]===0xa25&&picked.character.items[3]===0xa2a&&picked.character.stats[3]===30&&picked.character.inventory_revision===beforePickup.character.inventory_revision+1);
  await drag({x:766,y:384},{x:722,y:428});
  const unequipped=await record('ring-unequipped');
  game.check('unequipping restores exact base defense',unequipped.character.items[3]===-1&&unequipped.character.items[6]===0xa2a&&unequipped.character.stats[3]===initial.character.base_stats[3]);
  await game.act({keys:[],duration_ms:700},{name:'inventory-after',when:()=>true});
  game.check('same living character throughout',unequipped.lifecycle.alive&&unequipped.session.character===initial.session.character);
  await double({x:634,y:428});
  game.check('lesser ring equips for comparison',(await game.observe()).character.items[3]===0xa25);
  const browser=config.native.find(n=>n.actor===game.actor);
  const cdp=await connect({port:browser.port,target:browser.target,url:config.origin+'/',helper:config.browserTools});
  try{
    const rect=await cdp.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()');
    assert(rect.width===800&&rect.height===600);
    for(const [name,x,y] of [['unusable-sword',678,428],['ring-comparison',722,428],['untiered-spell',722,472],['three-dose-elixir',766,472]]){
      let moved=false;
      const start=(await game.observe()).game_ms;
      const action=game.act({keys:[],duration_ms:1600},{name,when:view=>moved&&view.game_ms>=start+800});
      const hover=(async()=>{
        const deadline=Date.now()+5000;
        while((await game.observe()).paused){assert(Date.now()<deadline,'Hover action must resume');await delay(20);}
        await cdp.call('Input.dispatchMouseEvent',{type:'mouseMoved',x:rect.x+x,y:rect.y+y});moved=true;
      })();
      const results=await Promise.allSettled([action,hover]);for(const r of results)if(r.status==='rejected')throw r.reason;
    }
  }finally{cdp.close();}
  const retained=await record('before-transfer');
  await game.act({keys:['Insert'],duration_ms:700});
  const reconnected=await record('reconnected');
  game.check('ordinary Nexus reconnect preserves equipment stats inventory revision and living identity',reconnected.map.name==='Nexus'&&reconnected.lifecycle.alive&&reconnected.session.character===initial.session.character&&JSON.stringify(reconnected.character.items)===JSON.stringify(retained.character.items)&&JSON.stringify(reconnected.character.base_stats)===JSON.stringify(retained.character.base_stats)&&reconnected.character.inventory_revision===retained.character.inventory_revision);
}
