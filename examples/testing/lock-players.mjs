import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';
export const fixture={class:0x30e,level:20,seed:908,world:-2,position:{x:42.5,y:20.5},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,50]};
export async function run(game){
  const out=join(process.env.QA_RESULTS_DIR,'native'),records=[];
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  function lists(){return JSON.parse(execFileSync('python3',['-c',"import sqlite3,json,sys; db=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True); print(json.dumps(list(db.execute('SELECT account,list_id,target FROM account_lists ORDER BY account,list_id,target'))))",config.state+'/game.db'],{encoding:'utf8'}));}
  async function save(label,g=game){const s=await g.inspect();records.push({label,lists:lists(),...s});await writeFile(join(out,'locks.json'),JSON.stringify(records,null,2));return s;}
  async function shot(name,g=game){const start=(await g.observe()).game_ms;await g.act({keys:[],duration_ms:1200},{name,when:v=>v.game_ms>=start+600});}
  async function click(x,y,g=game){await g.act({keys:[],mouse:{x,y},duration_ms:250});}
  async function walk(p,g=game){for(let n=0;n<100;n++){const v=await g.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y,d=Math.hypot(dx,dy);if(d<.4)return;const x=dx-dy,y=dx+dy,keys=[];if(Math.abs(x)>.12)keys.push(x>0?'d':'a');if(Math.abs(y)>.12)keys.push(y>0?'s':'w');await g.act({keys,duration_ms:Math.min(1000,Math.max(50,Math.round(d*70)))});}assert.fail('Lock case walking target not reached');}
  const initial=await save('initial'),pos=initial.view.position;
  const peer=await game.join({...fixture,position:{x:pos.x+3,y:pos.y}});
  const near=await game.join({...fixture,position:{x:pos.x+1,y:pos.y}});
  const peerInitial=await save('peer-initial',peer),nearInitial=await save('near-initial',near);
  const owner=initial.view.session.account,target=peerInitial.view.session.account;
  const locked=()=>JSON.stringify(lists())===JSON.stringify([[owner,0,target]]);
  await shot('locks-before');
  game.check('three distinct owned native accounts start with empty lists',new Set([owner,target,nearInitial.view.session.account]).size===3&&lists().length===0);
  await click(745,515);await shot('locks-menu');
  await click(590,480);await shot('locks-cancelled');
  game.check('leaving the Lock menu does not persist a lock',lists().length===0);
  // Six visible slots are not a saved-list cap; selected sorting is lock, distance, object ID: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/_ez.as
  await walk({x:pos.x+56,y:pos.y},peer);await shot('locks-unlocked-distant');await save('unlocked-distant');
  game.check('ordinary distant peer remains in this world without a persisted lock',Math.hypot((await peer.observe()).position.x-pos.x,(await peer.observe()).position.y-pos.y)>50&&lists().length===0);
  await walk({x:pos.x+3,y:pos.y},peer);await game.act({keys:[],duration_ms:700});
  await click(745,515);await click(650,420);await shot('locks-after');await save('locked');
  game.check('native Lock persists exactly the selected peer account only for its owner',locked());
  await shot('locks-peer-view',peer);await save('peer-after-lock',peer);
  await walk({x:pos.x+56,y:pos.y},peer);await shot('locks-locked-distant');await save('locked-distant');
  game.check('locked peer crosses the ordinary cutoff without changing the lock or living character',locked()&&Math.hypot((await peer.observe()).position.x-pos.x,(await peer.observe()).position.y-pos.y)>50&&(await peer.observe()).session.character===peerInitial.view.session.character);
  await click(645,515);await shot('locks-unlock-menu');await click(590,480);
  await walk({x:78.5,y:20.5});await walk({x:78.5,y:76.5});
  const portal=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0x720);assert(portal);await walk(portal.pos);
  await click(700,575);for(let n=0;n<12&&(await game.observe()).world!=='Vault';n++)await game.act({keys:[],duration_ms:500});
  const vault=await save('private-vault');await shot('locks-other-world');
  game.check('locking does not pull the peer into the owner private world',vault.view.map.name==='Vault'&&(await peer.observe()).world==='Nexus'&&locked());
  await game.act({keys:['Insert'],duration_ms:1000});for(let n=0;n<12&&(await game.observe()).world!=='Nexus';n++)await game.act({keys:[],duration_ms:500});
  await walk({x:84.5,y:72.5});await shot('locks-reconnected');const returned=await save('reconnected');
  game.check('ordinary Vault reconnect retains account character inventory and exact one-way lock',returned.view.map.name==='Nexus'&&returned.view.session.player!==initial.view.session.player&&returned.view.session.account===owner&&returned.view.session.character===initial.view.session.character&&JSON.stringify(returned.view.character.items)===JSON.stringify(initial.view.character.items)&&locked());
  await click(645,515);await click(550,420);await shot('locks-unlocked');await save('unlocked');
  game.check('native Unlock removes only the owners lock',lists().length===0);
  for(const [g,start] of [[game,initial],[peer,peerInitial],[near,nearInitial]]){const final=await save('final-'+g.actor,g);game.check('native participant '+g.actor+' remains alive with its original account character and items',final.view.lifecycle.alive&&final.view.session.account===start.view.session.account&&final.view.session.character===start.view.session.character&&JSON.stringify(final.view.character.items)===JSON.stringify(start.view.character.items));}
}
