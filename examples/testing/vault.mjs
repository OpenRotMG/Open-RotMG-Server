import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';
const stock=[0xa22,0xa23,0xa25,0xa2a,0xa4a,0xa3c,0xc0c,0xb3e];
export const fixture={class:0x30e,level:20,seed:898,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:stock,stats:[670,385,50,25,50,50,40,75]};
export async function run(game){
  const out=join(process.env.QA_RESULTS_DIR,'native'),records=[];
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const same=(a,b)=>JSON.stringify(a)===JSON.stringify(b);
  async function save(label,g=game){const s=await g.inspect();records.push({label,...s});await writeFile(join(out,'vault.json'),JSON.stringify(records,null,2));return s;}
  async function walk(p,g=game){for(let n=0;n<80;n++){const v=await g.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y,d=Math.hypot(dx,dy);if(d<.45)return;const x=dx-dy,y=dx+dy,keys=[];if(Math.abs(x)>.12)keys.push(x>0?'d':'a');if(Math.abs(y)>.12)keys.push(y>0?'s':'w');await g.act({keys,duration_ms:Math.min(250,Math.max(50,Math.round(d*70)))});}assert.fail('Vault walking target not reached');}
  async function enter(g=game){const before=await g.observe(),portal=(await g.inspect()).diagnostic.entities.find(e=>e.kind===0x720);assert(portal);await walk(portal.pos,g);await g.act({keys:[],mouse:{x:700,y:575},duration_ms:700});for(let n=0;n<10&&(await g.observe()).session.world===before.session.world;n++)await g.act({keys:[],duration_ms:500});assert.equal((await g.observe()).world,'Vault');}
  const drag=(from,to)=>game.act({keys:[],duration_ms:400,drag:{from,to}});
  const bag=i=>({x:634+44*(i%4),y:428+44*Math.floor(i/4)}),chest=i=>({x:634+44*(i%4),y:528+44*Math.floor(i/4)});
  const initial=await save('nexus');await enter();
  const arrived=await save('vault-initial'),open=arrived.diagnostic.entities.filter(e=>e.kind===0x504),closed=arrived.diagnostic.entities.filter(e=>e.kind===0x505);
  game.check('normal Nexus portal retains living account character and stock',arrived.view.lifecycle.alive&&arrived.view.session.account===initial.view.session.account&&arrived.view.session.character===initial.view.session.character&&same(arrived.view.character.items,initial.view.character.items));
  game.check('new account has one empty eight-slot chest and nineteen 500 Gold sellers',open.length===1&&same(open[0].items,Array(8).fill(-1))&&closed.length===19&&closed.every(e=>e.extra_stats.some(([k,v])=>k===36&&v===500)&&e.extra_stats.some(([k,v])=>k===40&&v===0)));
  await walk(open[0].pos);await game.act({keys:[],duration_ms:1200},{name:'vault-before',when:()=>true});
  // The selected client promises eight shared slots, not character-local storage: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/ClosedVaultChest.as
  for(let i=0;i<8;i++)await drag(bag(i),chest(i));
  const full=await save('eight-deposits'),stored=full.diagnostic.entities.find(e=>e.id===open[0].id);
  game.check('all eight native deposits commit exactly once',same(stored.items,stock)&&full.view.character.items.slice(4).every(i=>i===-1)&&full.view.character.inventory_revision===arrived.view.character.inventory_revision+8);
  await drag(chest(0),{x:634,y:384});const refused=await save('incompatible');
  game.check('incompatible equipped withdrawal is atomic',same(refused.view.character.items,full.view.character.items)&&refused.view.character.inventory_revision===full.view.character.inventory_revision&&same(refused.diagnostic.entities.find(e=>e.id===open[0].id).items,stock));
  await drag(chest(0),chest(7));const reordered=[...stock];[reordered[0],reordered[7]]=[reordered[7],reordered[0]];
  await drag(chest(0),bag(0));const withdrawn=await save('rearranged-withdrawal');reordered[0]=-1;
  game.check('native chest rearrangement and withdrawal conserve items',same(withdrawn.diagnostic.entities.find(e=>e.id===open[0].id).items,reordered)&&withdrawn.view.character.items[4]===stock[7]&&withdrawn.view.character.inventory_revision===full.view.character.inventory_revision+2);
  await game.act({keys:['Insert'],duration_ms:700});assert.equal((await game.observe()).world,'Nexus');await enter();
  const returned=await save('reentered');game.check('ordinary Nexus reconnect preserves chest contents and inventory',same(returned.diagnostic.entities.find(e=>e.kind===0x504).items,reordered)&&same(returned.view.character.items,withdrawn.view.character.items)&&returned.view.session.character===initial.view.session.character&&returned.view.character.inventory_revision===withdrawn.view.character.inventory_revision);
  const peer=await game.join({...fixture,backpack:Array(8).fill(-1)});await enter(peer);const privatePeer=await save('other-account',peer);
  game.check('second native account enters a different private Vault with no shared items',privatePeer.view.session.account!==returned.view.session.account&&privatePeer.view.session.world!==returned.view.session.world&&same(privatePeer.diagnostic.entities.find(e=>e.kind===0x504).items,Array(8).fill(-1)));
  await walk(returned.diagnostic.entities.find(e=>e.kind===0x504).pos);await game.act({keys:[],duration_ms:1200},{name:'vault-after',when:()=>true});
  const durable=JSON.parse(execFileSync('python3',['-c',`import json,sqlite3,sys
with sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True) as db:
 print(json.dumps({'vault':[(a,b,json.loads(c),d) for a,b,c,d in db.execute('SELECT account,chest,items,revision FROM vaults ORDER BY account,chest')], 'integrity':db.execute('PRAGMA integrity_check').fetchone()[0],'foreign_keys':db.execute('PRAGMA foreign_key_check').fetchall()}))`,config.state+'/game.db'],{encoding:'utf8'}));
  game.check('durable account chest identities and exact contents agree with native views',durable.integrity==='ok'&&durable.foreign_keys.length===0&&durable.vault.length===2&&durable.vault.some(([a,c,items])=>a===initial.view.session.account&&c===0&&same(items,reordered))&&durable.vault.some(([a,c,items])=>a===privatePeer.view.session.account&&c===0&&same(items,Array(8).fill(-1))));
  await writeFile(join(out,'vault-durable.json'),JSON.stringify(durable,null,2));
  const seller=returned.diagnostic.entities.find(e=>e.kind===0x505);assert(seller);await walk(seller.pos);
  const beforeBuy=await save('guest-buy-before'),buyStart=(await game.observe()).game_ms;
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1800},{name:'vault-guest-refusal',when:v=>v.game_ms>=buyStart+700});
  const guestRefusal=await save('guest-buy-refused');
  game.check('guest purchase leaves chest ownership inventory and revision unchanged',same(guestRefusal.view.character.items,beforeBuy.view.character.items)&&guestRefusal.view.character.inventory_revision===beforeBuy.view.character.inventory_revision&&guestRefusal.diagnostic.entities.filter(e=>e.kind===0x504).length===1&&guestRefusal.diagnostic.entities.some(e=>e.id===seller.id&&e.kind===0x505));
}
