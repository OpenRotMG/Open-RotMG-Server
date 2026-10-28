import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';
import {execFileSync} from 'node:child_process';

const base=[100,100,12,0,10,15,0,0];
export const fixture={class:0x30e,level:20,seed:891,world:'ocean-trench',
  position:w=>({x:w.spawn_points[0][0],y:w.spawn_points[0][1]+2}),
  equipment:[0xa97,0xa2e,-1,-1],backpack:[0xb3e,0xb3e,0xa22,-1,-1,-1,-1,-1],stats:base};
export async function run(game){
  const out=process.argv[process.argv.indexOf('--run-dir')+1];
  const config=JSON.parse(await readFile(out+'/native.json','utf8'));
  const token=(await readFile(config.state+'/control.token','utf8')).trim();
  const same=(a,b)=>JSON.stringify(a)===JSON.stringify(b);
  const stats=v=>['max_hp','max_mp','attack','defense','speed','dexterity','vitality','wisdom'].map(k=>v.character.stats[k]);
  const start=await game.observe();
  game.check('backpack amulets grant no bonuses',same(stats(start),base));
  await game.act({keys:[],duration_ms:1200},{name:'amulet-before',when:()=>true});
  await game.act({keys:[],duration_ms:450,drag:{from:{x:634,y:428},to:{x:634,y:385}}});
  const refused=await game.observe();
  game.check('amulet refuses weapon slot atomically',same(refused.character.items,start.character.items)&&refused.character.inventory_revision===start.character.inventory_revision&&same(stats(refused),base));
  await game.act({keys:[],duration_ms:450,drag:{from:{x:634,y:428},to:{x:766,y:385}}});
  const equipped=await game.observe();
  game.check('native equip grants exactly twenty HP MP and two other stats',equipped.character.items[3]===0xb3e&&equipped.character.items[4]===-1&&equipped.character.inventory_revision===start.character.inventory_revision+1&&same(stats(equipped),base.map((n,i)=>n+(i<2?20:2))));
  await game.act({keys:[' '],duration_ms:250});
  game.check('native spell spends mana before resurrection',(await game.observe()).character.mp<equipped.character.mp);
  const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'spawn',spec:{...fixture,position:start.position,world:start.session.world,guid:'amulet-backpack-control',controller:'direct',profile:null}}})});
  const control=await r.json();assert(r.ok&&control.status==='succeeded',JSON.stringify(control));
  await game.act({keys:[],duration_ms:9000},{name:'amulet-suffocation',when:v=>v.world==='Ocean Trench'&&v.character.hp<60});
  const restored=await game.observe();
  game.check('fatal native suffocation returns same living character to Nexus',restored.world==='Nexus'&&restored.session.world!==start.session.world&&restored.lifecycle.alive&&restored.session.character===start.session.character);
  const retained=[...equipped.character.items];retained[3]=-1;
  game.check('only equipped amulet consumed and its bonuses removed',same(restored.character.items,retained)&&same(stats(restored),base)&&restored.character.inventory_revision===equipped.character.inventory_revision+1);
  game.check('post-consumption capacities fully restored',restored.character.hp===base[0]&&restored.character.mp===base[1]);
  game.check('XP level and living Fame retained',restored.character.level===start.character.level&&restored.character.experience===start.character.experience&&restored.character.fame===start.character.fame);
  await game.act({keys:['d'],duration_ms:350});
  const moved=await game.observe();
  game.check('same native character remains playable after forced reconnect',moved.position.x>restored.position.x&&same(moved.character.items,retained));
  await game.act({keys:[],duration_ms:1200},{name:'amulet-restored',when:v=>v.world==='Nexus'});
  const database=JSON.parse(execFileSync('python3',['-c',`
import json,sqlite3,sys
with sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True) as db:
 account=int(sys.argv[2]); character=int(sys.argv[3])
 row=db.execute('SELECT alive,items,stats FROM characters WHERE id=? AND account=?',(character,account)).fetchone()
 print(json.dumps({'alive':row[0],'items':json.loads(row[1]),'character':json.loads(row[2]),'deaths':db.execute('SELECT count(*) FROM deaths d JOIN characters c ON c.id=d.character WHERE c.account=?',(account,)).fetchone()[0],'wallet':db.execute('SELECT balance,earned FROM account_fame WHERE account=?',(account,)).fetchone(),'control':db.execute('SELECT c.alive,d.killed_by,c.items,d.snapshot FROM characters c JOIN deaths d ON d.character=c.id WHERE c.account!=?',(account,)).fetchall()}))
`,config.state+'/game.db',String(start.session.account),String(start.session.character)],{encoding:'utf8'}));
  game.check('resurrection persisted without death reward or Fame payout',database.alive===1&&same(database.items,retained)&&database.deaths===0&&same(database.wallet,[0,0]));
  game.check('backpack-only control dies once instead of resurrecting',database.control.length===1&&database.control[0][0]===0&&database.control[0][1]==='suffocation'&&JSON.parse(database.control[0][3]).items[4]===0xb3e);
  await writeFile(out+'/amulet.json',JSON.stringify({start,refused,equipped,restored,moved,control,database},null,2));
}
