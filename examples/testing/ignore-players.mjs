import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {execFileSync} from 'node:child_process';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:20,seed:909,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,60]};
export async function run(game){
  const out=join(process.env.QA_RESULTS_DIR,'native'),peer=await game.join(fixture),records=[];
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const setup=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:`Bearer ${token}`,'Content-Type':'application/json'},body:JSON.stringify({id:'ignore-prepared-guild',command:{op:'cheat',actor:game.actor,change:{kind:'guild',name:'Ignore Fixture',members:[peer.actor]}}})});
  const guild=await setup.json();assert(setup.ok&&guild.status==='succeeded',JSON.stringify(guild));
  await writeFile(join(out,'ignore-guild-fixture.json'),JSON.stringify(guild,null,2));
  const connections=await Promise.all(config.native.map(b=>connect({...b,url:config.origin+'/',helper:config.browserTools})));
  const cdp=g=>connections[config.native.findIndex(b=>b.actor===g.actor)];
  async function save(label,g=game){const s=await g.inspect();records.push({label,...s});await writeFile(join(out,'ignore.json'),JSON.stringify(records,null,2));return s;}
  function durable(){return JSON.parse(execFileSync('python3',['-c',`import json,sqlite3,sys
with sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True) as db:
 print(json.dumps({'lists':list(db.execute('SELECT account,list_id,target FROM account_lists ORDER BY account,list_id,target')),'integrity':db.execute('PRAGMA integrity_check').fetchone()[0],'foreign_keys':list(db.execute('PRAGMA foreign_key_check'))}))`,config.state+'/game.db'],{encoding:'utf8'}));}
  async function chat(g,text){
    const action=g.act({keys:[],duration_ms:2500});
    const typing=(async()=>{
      for(let n=0;(await g.observe()).paused;n++){assert(n<250,'Chat action did not unpause');await delay(20);}
      const enter=async()=>{for(const type of ['keyDown','keyUp'])await cdp(g).call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});};
      await enter();await cdp(g).call('Input.insertText',{text});await enter();
    })();
    const results=await Promise.allSettled([action,typing]);for(const r of results)if(r.status==='rejected')throw r.reason;
    await save(text,g);
  }
  async function capture(name,g=game){await g.act({keys:[],duration_ms:1200},{name,when:()=>true});}
  async function walk(p,g){for(let n=0;n<80;n++){const v=await g.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y,d=Math.hypot(dx,dy);if(d<.45)return;const x=dx-dy,y=dx+dy,keys=[];if(Math.abs(x)>.12)keys.push(x>0?'d':'a');if(Math.abs(y)>.12)keys.push(y>0?'s':'w');await g.act({keys,duration_ms:Math.min(250,Math.max(50,Math.round(d*70)))});}assert.fail('Vault walking target not reached');}
  try{
    const a=await save('initial'),b=await save('peer-initial',peer),owner=a.view.session.account,target=b.view.session.account,name=b.diagnostic.character.name;
    game.check('two distinct ordinary native accounts share Nexus',owner!==target&&a.view.session.world===b.view.session.world);
    await chat(peer,'Before ignore visible');await chat(peer,'/g Before ignore guild');await capture('ignore-before');
    await chat(game,'/ignore '+name.toLowerCase());
    game.check('case-insensitive ignore persists exactly one account in list1',JSON.stringify(durable().lists)===JSON.stringify([[owner,1,target]]));
    await chat(game,'/ignore '+name);game.check('duplicate ignore is idempotent',durable().lists.length===1);
    await chat(peer,'Hidden local message');await chat(peer,'/yell Hidden Nexus yell');await chat(peer,'/g Hidden guild message');await capture('ignore-muted');
    await chat(game,'Outgoing remains visible');await capture('ignore-peer-outgoing',peer);
    await chat(peer,'/trade '+a.diagnostic.character.name);await capture('ignore-trade-request');
    await chat(game,'/trade '+name);await capture('ignore-trade-open');await capture('ignore-peer-trade-open',peer);
    await game.act({keys:[],mouse:{x:650,y:575},duration_ms:500});await capture('ignore-trade-cancelled',peer);
    game.check('cancelled trade preserves both native inventories',JSON.stringify((await game.inspect()).view.character.items)===JSON.stringify(a.view.character.items)&&JSON.stringify((await peer.inspect()).view.character.items)===JSON.stringify(b.view.character.items));
    await chat(game,'/ignore '+a.diagnostic.character.name);await chat(game,'/ignore MissingPlayer');
    game.check('self and unknown targets do not mutate persisted list',JSON.stringify(durable().lists)===JSON.stringify([[owner,1,target]]));
    const portal=(await peer.inspect()).diagnostic.entities.find(e=>e.kind===0x720);assert(portal);
    await walk(portal.pos,peer);await peer.act({keys:[],mouse:{x:700,y:575},duration_ms:1200});
    game.check('ignored peer ordinarily enters private Vault', (await peer.observe()).world==='Vault');
    await chat(peer,'/g Hidden guild from Vault');await capture('ignore-private-guild');
    // Build123.3 explicitly permits private-area targets: https://web.archive.org/web/20130605124251id_/https://forums.wildshadow.com/node/100145
    await chat(game,'/unignore '+name);game.check('unignore resolves a player in another private world',durable().lists.length===0);
    await chat(game,'/ignore '+name);game.check('ignore resolves a player in another private world',durable().lists.some(r=>r[0]===owner&&r[1]===1&&r[2]===target));
    const ownPortal=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0x720);assert(ownPortal);await walk(ownPortal.pos,game);
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1200});
    const privateA=await save('owner-vault'),privateB=await save('peer-vault',peer);
    game.check('separate private Vaults retain both original living characters',privateA.view.map.name==='Vault'&&privateB.view.map.name==='Vault'&&privateA.view.session.world!==privateB.view.session.world&&privateA.view.session.character===a.view.session.character&&privateB.view.session.character===b.view.session.character&&privateA.view.lifecycle.alive&&privateB.view.lifecycle.alive);
    await game.act({keys:['Insert'],duration_ms:1400});await peer.act({keys:['Insert'],duration_ms:1400});
    game.check('normal reconnect retains account ignore row',durable().lists.some(r=>r[0]===owner&&r[2]===target)&&(await game.observe()).world==='Nexus'&&(await peer.observe()).world==='Nexus');
    await chat(peer,'Hidden after reconnect');await capture('ignore-reconnected');
    await chat(game,'/unignore '+name);await chat(peer,'After unignore visible');await chat(peer,'/yell After unignore yell');await chat(peer,'/g After unignore guild');await capture('ignore-after');
    for(const [add,label] of [[true,'ignore'],[false,'unignore']]){
      await game.act({keys:[],mouse:{x:650,y:518},duration_ms:300});await capture('ignore-menu-'+label);
      await game.act({keys:[],mouse:{x:550,y:480},duration_ms:500});
      game.check('native player menu persists '+label,JSON.stringify(durable().lists)===JSON.stringify(add?[[owner,1,target]]:[]));
    }
    const final=durable();game.check('unignore deletes durable relation without corrupting account state',final.lists.length===0&&final.integrity==='ok'&&final.foreign_keys.length===0);
    const events=(await readFile(join(out,'server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
    const deliveries=events.filter(e=>e.message==='public chat delivered').map(e=>[e.account,e.recipients]);
    game.check('actual chat delivery is suppressed only toward the ignoring recipient, including after reconnect',JSON.stringify(deliveries)===JSON.stringify([[target,2],[target,1],[target,1],[owner,2],[target,1],[target,2],[target,2]]));
    game.check('ignore does not block reciprocal native trade and ordinary cancellation',events.some(e=>e.message==='native trade started')&&events.some(e=>e.message==='trade cancelled without inventory mutation'));
    const guildDeliveries=events.filter(e=>e.message==='guild chat delivered').map(e=>e.recipients);
    game.check('native guild chat is suppressed locally and across private worlds until unignore',JSON.stringify(guildDeliveries)===JSON.stringify([2,1,1,2]));
    await writeFile(join(out,'ignore-durable.json'),JSON.stringify({final,deliveries,guildDeliveries},null,2));
  }finally{for(const c of connections)c.close();}
}
