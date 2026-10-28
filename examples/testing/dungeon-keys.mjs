import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const output=name=>join(process.env.QA_RESULTS_DIR,name);
export const fixture={class:0x30e,level:20,seed:890,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:[0x701,-1,-1,-1,-1,-1,-1,-1],stats:null};
const keys=[[0x701,0x71a,'Undead Lair'],[0x705,0x717,'Pirate Cave'],[0x706,0x719,'Spider Den'],[0x70a,0x71b,'Abyss of Demons'],[0x70b,0x718,'Snake Pit'],[0x710,0x72c,'Tomb of the Ancients'],[0x71f,0x70c,'Sprite World'],[0xc11,0x730,'Ocean Trench'],[0xc19,0x733,'Forbidden Jungle'],[0xc23,0x739,'Manor of the Immortals'],[0xc2e,0x741,"Davy Jones's Locker"],[0xc3d,0x742,'Beachzone'],[0xc2f,0x890,'Mad Lab']];
export async function run(game){
  const config=JSON.parse(await readFile(output('native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),observations=[];
  async function supply(item){const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'item',slot:4,item}}})});const r=await response.json();assert(response.ok&&r.status==='succeeded',JSON.stringify(r));await game.act({keys:[],duration_ms:100});}
  async function save(name,g=game){const s=await g.inspect();observations.push({name,state:s});await writeFile(output('keys.json'),JSON.stringify(observations));return s;}
  async function enter(g,from,name){await g.act({keys:[],mouse:{x:700,y:575},duration_ms:700});for(let n=0;n<10&&(await g.observe()).session.world===from;n++)await g.act({keys:[],duration_ms:500});const s=await save(name,g);assert.notEqual(s.view.session.world,from,'Native portal admission failed: '+name);return s;}
  async function escape(){await game.act({keys:['Insert'],duration_ms:700});for(let n=0;n<10&&(await game.observe()).world!=='Nexus';n++)await game.act({keys:[],duration_ms:500});assert.equal((await game.observe()).world,'Nexus');}
  async function walk(p){for(let n=0;n<80;n++){const v=await game.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y,d=Math.hypot(dx,dy);if(d<.6)return;const x=dx-dy,y=dx+dy,ks=[];if(Math.abs(x)>.15)ks.push(x>0?'d':'a');if(Math.abs(y)>.15)ks.push(y>0?'s':'w');await game.act({keys:ks,duration_ms:Math.min(250,Math.max(50,Math.round(d*80)))});}assert.fail('Native walking target not reached');}
  const initial=await save('initial');
  await game.act({keys:[],duration_ms:1200},{name:'keys-before',when:()=>true});
  await game.act({keys:['d'],duration_ms:800});
  for(const [key,kind,name] of keys){
    await supply(key);const before=await save(name+'-before');
    // Kithio Create places the selected portal at the user's position; it does not impose a world allowlist: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.UseItem.cs#L678-L702
    await game.act({keys:['1'],duration_ms:700});const after=await save(name+'-opened'),a=after.view.character,b=before.view.character;
    const portal=after.diagnostic.entities.find(e=>e.kind===kind&&e.published&&!before.diagnostic.entities.some(old=>old.id===e.id));assert(portal,'Selected key portal missing: '+name);
    game.check(name+' key commits once without changing stats MP or other slots',a.items[4]===-1&&a.inventory_revision===b.inventory_revision+1&&a.mp===b.mp&&JSON.stringify(a.stats)===JSON.stringify(b.stats)&&JSON.stringify(a.base_stats)===JSON.stringify(b.base_stats)&&a.items.every((item,slot)=>slot===4||item===b.items[slot]));
    game.check(name+' portal appears at native user position',Math.hypot(portal.pos.x-before.view.position.x,portal.pos.y-before.view.position.y)<.05);
    await game.act({keys:['1'],duration_ms:100});game.check(name+' empty slot cannot consume twice',(await game.observe()).character.inventory_revision===a.inventory_revision);
  }
  const opened=await save('expiry-start'),portal=opened.diagnostic.entities.find(e=>e.kind===0x70c);assert(portal);
  for(let n=0;n<3;n++)await game.act({keys:[],duration_ms:10000});
  game.check('key portal remains published beyond30 seconds',(await game.inspect()).diagnostic.entities.some(e=>e.id===portal.id&&e.published));
  for(let n=0;n<3;n++)await game.act({keys:[],duration_ms:10000});
  const expired=await save('expired');game.check('all13 key portals disappear after one minute',keys.every(([,kind])=>!expired.diagnostic.entities.some(e=>e.kind===kind)));
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});game.check('expired portal cannot admit or refund',((await game.observe()).session.world===expired.view.session.world)&&(await game.observe()).character.items[4]===-1);
  await supply(0x71f);await game.act({keys:['1'],duration_ms:700},{name:'sprite-key-opened',when:v=>v.character.items[4]===-1});const shared=await save('shared-open');
  const peer=await game.join({...fixture,world:shared.view.session.world,position:shared.view.position,backpack:Array(8).fill(-1)});
  await peer.act({keys:[],duration_ms:300});const other=await enter(peer,shared.view.session.world,'peer-sprite'),owner=await enter(game,shared.view.session.world,'owner-sprite');
  game.check('another account and key owner enter the same Sprite instance',owner.view.map.name==='Sprite World'&&other.view.map.name==='Sprite World'&&owner.view.session.world===other.view.session.world&&owner.view.session.account!==other.view.session.account);
  await game.act({keys:[],duration_ms:1200},{name:'shared-sprite',when:()=>true});await escape();
  const nexus=await game.inspect(),vault=nexus.diagnostic.entities.find(e=>e.kind===0x720);assert(vault);await walk(vault.pos);const privateWorld=await enter(game,nexus.view.session.world,'private-vault');assert.equal(privateWorld.view.map.name,'Vault');
  await game.act({keys:['d'],duration_ms:800});await supply(0x705);const vaultBefore=await game.observe();await game.act({keys:['1'],duration_ms:700});const privateEntry=await enter(game,vaultBefore.session.world,'vault-key-pirate');
  game.check('key in private Vault creates an enterable Pirate Cave and consumes once',privateEntry.view.map.name==='Pirate Cave'&&privateEntry.view.character.items[4]===-1&&privateEntry.view.character.inventory_revision===vaultBefore.character.inventory_revision+1);
  await escape();const final=await save('reconnected');game.check('ordinary reconnect preserves living identity and consumed inventory',final.view.lifecycle.alive&&final.view.session.character===initial.view.session.character&&final.view.character.items.slice(4).every(i=>i===-1));
  await supply(0x722);const refused=await save('incantation-before');await game.act({keys:['1'],duration_ms:300});const retained=await save('incantation-refused');
  game.check('Incantation outside cleared Chamber is retained without portal or revision change',retained.view.character.items[4]===0x722&&retained.view.character.inventory_revision===refused.view.character.inventory_revision&&!retained.diagnostic.entities.some(e=>e.kind===0x700));
}
