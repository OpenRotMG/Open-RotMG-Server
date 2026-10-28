import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const kinds=[0xae9,0xaea,0xa1f,0xa20,0xa21,0xa4c,0xa34,0xa35];
const caps=[670,385,75,25,50,75,40,60];
export const fixture={class:0x30e,level:20,seed:882,world:'pirate-cave',position:'spawn',equipment:[-1,-1,-1,0xa4f],backpack:kinds,stats:caps.map((cap,i)=>cap-(i<2?2:1))};
export async function run(game){
  const root=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),observations=[];
  async function prepare(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function snapshot(name){const view=await game.observe();observations.push({name,view});return view;}
  await game.act({keys:[],duration_ms:200});
  const initial=await snapshot('before');
  for(let i=0;i<8;i++){
    const before=await snapshot('before-'+i);
    await game.act({keys:[String(i+1)],duration_ms:150});
    const after=await snapshot('consumed-'+i),expected=[...before.character.base_stats];expected[i]=caps[i];
    game.check(kinds[i]+': native drink clips only the selected base stat to its class cap',JSON.stringify(after.character.base_stats)===JSON.stringify(expected));
    game.check(kinds[i]+': exactly one item and revision committed',after.character.items[i+4]===-1&&after.character.inventory_revision===before.character.inventory_revision+1);
  }
  await game.act({keys:[],duration_ms:1000},{name:'stat-potions-capped',when:()=>true});
  for(let i=0;i<8;i++)await prepare({kind:'item',slot:i+4,item:kinds[i]});
  const capped=await snapshot('refilled-at-cap');
  game.check('class cap excludes the equipped Defense ring bonus',capped.character.stats.defense===26&&capped.character.base_stats[3]===25&&capped.character.items[3]===0xa4f);
  for(let i=0;i<8;i++){
    await game.act({keys:[String(i+1)],duration_ms:150});
    const after=await snapshot('refused-'+i);
    game.check(kinds[i]+': cap refusal preserves all items, base stats and revision',JSON.stringify(after.character.items)===JSON.stringify(capped.character.items)&&JSON.stringify(after.character.base_stats)===JSON.stringify(caps)&&after.character.inventory_revision===capped.character.inventory_revision);
  }
  await prepare({kind:'item',slot:4,item:0xa22});
  await prepare({kind:'item',slot:5,item:0xa23});
  await prepare({kind:'vitals',hp:660,mp:375});
  await game.act({keys:['1'],duration_ms:1000},{name:'partial-health',when:view=>view.character.items[4]===-1});
  await game.act({keys:['2'],duration_ms:1000},{name:'partial-mana',when:view=>view.character.items[5]===-1});
  const restored=await snapshot('partially-restored');
  game.check('native restorative potions fill, but never exceed, actual maxima',restored.character.hp===670&&restored.character.mp===385&&restored.character.items[4]===-1&&restored.character.items[5]===-1);
  await prepare({kind:'item',slot:4,item:0xc3a});
  await game.act({keys:['1'],duration_ms:150});
  const boosted=await snapshot('boosted');
  game.check('native Blue Paradise adds temporary maxima without modifying capped base stats',boosted.character.stats.max_hp===720&&boosted.character.stats.max_mp===435&&JSON.stringify(boosted.character.base_stats)===JSON.stringify(caps)&&boosted.character.items[4]===-1);
  for(let i=0;i<16;i++)await game.act({keys:[],duration_ms:2000});
  const expired=await snapshot('expired');
  game.check('temporary maxima expire without removing permanent gains',expired.character.stats.max_hp===670&&expired.character.stats.max_mp===385&&JSON.stringify(expired.character.base_stats)===JSON.stringify(caps));
  const retained=await snapshot('before-reconnect');
  await game.act({keys:['Insert'],duration_ms:500});
  const reconnected=await snapshot('reconnected');
  game.check('ordinary Nexus reconnect preserves committed gains, inventory and living character',reconnected.world==='Nexus'&&reconnected.lifecycle.alive&&reconnected.session.character===initial.session.character&&JSON.stringify(reconnected.character.base_stats)===JSON.stringify(caps)&&JSON.stringify(reconnected.character.items)===JSON.stringify(retained.character.items));
  await writeFile(join(root,'stat-potions-observations.json'),JSON.stringify({preparedStatsAndItems:true,observations},null,2));
}
