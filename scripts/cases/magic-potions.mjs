import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';
const output=name=>join(process.env.QA_RESULTS_DIR,name);
export const fixture={class:0x30e,level:20,seed:881,world:'pirate-cave',position:'spawn',equipment:[-1,0xa2e,-1,-1],backpack:[0xadd,0xa23,0xaec,-1,-1,-1,-1,-1],stats:[670,385,50,0,50,50,0,0]};
export async function run(game){
  const config=JSON.parse(await readFile(output('native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),observations=[];
  async function prepare(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function snapshot(name){const state=await game.inspect();observations.push({name,state});return state;}
  const protect=()=>prepare({kind:'protection',condition:'invulnerable',duration_ms:60000});
  const browser=config.native.find(n=>n.actor===game.actor);
  const cdp=await connect({...browser,url:config.origin+'/',helper:config.browserTools});
  async function groundDoubleClick(amount){
    // A selected-client double click must not include the controller's per-action readback (inventory-equipment.mjs:19–34).
    const action=game.act({keys:[],duration_ms:1200},{name:`magic-${amount}-ground`,when:v=>v.character.mp>=amount+10});
    const input=(async()=>{
      const deadline=Date.now()+5000;
      while((await game.observe()).paused||!(await cdp.eval('document.querySelector("ruffle-player").ruffle().isPlaying'))){assert(Date.now()<deadline,'Ground click must resume');await delay(20);}
      await cdp.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
      const rect=await cdp.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return {x:r.x,y:r.y,w:r.width,h:r.height};})()');
      assert(rect.w===800&&rect.h===600,'Native viewport must be unscaled');
      const point={x:rect.x+634,y:rect.y+528},times=[];
      await cdp.call('Input.dispatchMouseEvent',{type:'mouseMoved',...point});
      for(let n=0;n<2;n++){
        times.push(performance.now());
        try{await cdp.call('Input.dispatchMouseEvent',{type:'mousePressed',button:'left',buttons:1,clickCount:1,...point});}
        finally{await cdp.call('Input.dispatchMouseEvent',{type:'mouseReleased',button:'left',buttons:0,clickCount:1,...point});}
      }
      observations.push({name:'ground-double-click',amount,gap_ms:times[1]-times[0]});
      assert(times[1]-times[0]<250,'Native ground click pair exceeded the selected client timer');
    })();
    for(const r of await Promise.allSettled([action,input]))if(r.status==='rejected')throw r.reason;
  }
  try{
  const initial=await snapshot('initial');
  await protect();
  await game.act({keys:[],duration_ms:1200},{name:'magic-potions-before',when:()=>true});
  await game.act({keys:['d'],duration_ms:800});
  const moved=await game.observe();
  game.check('native movement leaves the entrance portal so dropped bags can open',Math.hypot(moved.position.x-initial.view.position.x,moved.position.y-initial.view.position.y)>2);
    for(const [kind,amount] of [[0xadd,50],[0xa23,100],[0xaec,200]]){
      for(const mp of [10,380,385]){
        if(!((await game.observe()).character.conditions&(1<<24)))await protect();
        await prepare({kind:'item',slot:4,item:kind});await prepare({kind:'vitals',hp:670,mp});
        await game.act({keys:[],duration_ms:100});const before=await snapshot(`${kind}-${mp}-before`);
        // The compiled comparator spends full-MP consumables too: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.UseItem.cs#L88-L102
        await game.act({keys:['1'],duration_ms:1200},mp===10?{name:`magic-${amount}-restored`,when:v=>v.character.items[4]===-1}:undefined);
        const after=await snapshot(`${kind}-${mp}-after`),a=after.view.character,b=before.view.character;
        game.check(`${amount} MP potion consumes exactly once at starting MP ${mp}`,a.items[4]===-1&&a.inventory_revision===b.inventory_revision+1);
        game.check(`${amount} MP potion restores its amount capped at385 at starting MP ${mp}`,a.mp>=Math.min(385,b.mp+amount)&&a.mp<=Math.min(385,b.mp+amount)+2);
        game.check(`${amount} MP potion preserves health, stats and other slots at starting MP ${mp}`,a.hp===b.hp&&JSON.stringify(a.base_stats)===JSON.stringify(b.base_stats)&&JSON.stringify(a.stats)===JSON.stringify(b.stats)&&a.items.every((item,slot)=>slot===4||item===b.items[slot]));
        await game.act({keys:['1'],duration_ms:100});const empty=await game.observe();
        game.check(`${amount} MP empty-slot reuse cannot consume twice at starting MP ${mp}`,empty.character.inventory_revision===a.inventory_revision&&empty.character.mp<=a.mp+1);
      }
      if(!((await game.observe()).character.conditions&(1<<24)))await protect();
      await prepare({kind:'item',slot:4,item:kind});
      await game.act({keys:[],duration_ms:100});
      await game.act({keys:[],duration_ms:500,drag:{from:{x:634,y:428},to:{x:300,y:300}}});
      const dropped=await snapshot(`${kind}-dropped`),bag=dropped.diagnostic.entities.find(e=>e.items?.includes(kind));
      assert(bag,'Native drop did not create potion bag');
      game.check(`${amount} MP drop has selected public/soulbound bag`,bag.kind===(kind===0xaec?0x503:0x500));
      for(let slot=4;slot<12;slot++)await prepare({kind:'item',slot,item:0xa22});
      await prepare({kind:'vitals',hp:670,mp:10});
      await game.act({keys:[],duration_ms:300});const before=await snapshot(`${kind}-ground-before`);
      await groundDoubleClick(amount);
      const after=await snapshot(`${kind}-ground-after`),a=after.view.character,b=before.view.character;
      const passive=(after.view.time_ms-before.view.time_ms)*0.0006;
      game.check(`${amount} MP ground use restores without pickup`,a.mp-b.mp-amount>=Math.floor(passive)&&a.mp-b.mp-amount<=Math.ceil(passive)&&a.hp===b.hp&&JSON.stringify(a.stats)===JSON.stringify(b.stats)&&JSON.stringify(a.base_stats)===JSON.stringify(b.base_stats)&&JSON.stringify(a.items)===JSON.stringify(b.items)&&a.inventory_revision===b.inventory_revision+1);
      game.check(`${amount} MP ground source consumed once`,!after.diagnostic.entities.some(e=>e.id===bag.id&&e.items?.includes(kind)));
    }
    const retained=await snapshot('before-reconnect');
    await game.act({keys:['Insert'],duration_ms:700});
    await game.act({keys:[],duration_ms:1200},{name:'magic-potions-reconnected',when:v=>v.world==='Nexus'});
    const after=await snapshot('reconnected');
    game.check('ordinary Nexus reconnect retains same living character and spent inventory',after.view.map.name==='Nexus'&&after.view.lifecycle.alive&&after.view.session.character===initial.view.session.character&&JSON.stringify(after.view.character.items)===JSON.stringify(retained.view.character.items)&&after.view.character.inventory_revision===retained.view.character.inventory_revision);
  }finally{cdp.close();await writeFile(output('potion-observations.json'),JSON.stringify({preparedItemsStatsVitals:true,quietEvidence:'PR132; not reverified by this case',observations},null,2));}
}
