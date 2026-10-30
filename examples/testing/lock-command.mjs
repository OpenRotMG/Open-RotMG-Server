import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:20,seed:1031,world:-2,position:{x:78.5,y:76.5},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,50]};
export async function run(game){
  const out=join(process.env.QA_RESULTS_DIR,'native'),records=[];
  const peer=await game.join({...fixture,position:{x:79.5,y:76.5}});
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const cdp=await connect({...config.native.find(b=>b.actor===game.actor),url:config.origin+'/',helper:config.browserTools});
  function lists(){return JSON.parse(execFileSync('python3',['-c',"import sqlite3,json,sys; db=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True); print(json.dumps(list(db.execute('SELECT account,list_id,target FROM account_lists ORDER BY account,list_id,target'))))",config.state+'/game.db'],{encoding:'utf8'}));}
  async function save(label){records.push({label,lists:lists(),owner:await game.inspect(),peer:await peer.inspect()});await writeFile(join(out,'lock-command.json'),JSON.stringify(records,null,2));}
  async function shot(name,g=game){await g.act({keys:[],duration_ms:1200},{name,when:()=>true});}
  async function chat(text){
    const action=game.act({keys:[],duration_ms:2500});
    const typing=(async()=>{
      for(let n=0;(await game.observe()).paused;n++){assert(n<250,'Chat action did not unpause');await delay(20);}
      const enter=async()=>{for(const type of ['keyDown','keyUp'])await cdp.call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});};
      await enter();await cdp.call('Input.insertText',{text});await enter();
    })();
    const results=await Promise.allSettled([action,typing]);for(const r of results)if(r.status==='rejected')throw r.reason;
    await save(text);
  }
  async function walk(p){for(let n=0;n<60;n++){const v=await game.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y,d=Math.hypot(dx,dy);if(d<.4)return;const x=dx-dy,y=dx+dy,keys=[];if(Math.abs(x)>.12)keys.push(x>0?'d':'a');if(Math.abs(y)>.12)keys.push(y>0?'s':'w');await game.act({keys,duration_ms:Math.min(500,Math.max(50,Math.round(d*70)))});}assert.fail('Native Vault walk blocked');}
  try{
    const initial=await game.inspect(),other=await peer.inspect(),owner=initial.view.session.account,target=other.view.session.account,name=other.diagnostic.character.name;
    const locked=()=>JSON.stringify(lists())===JSON.stringify([[owner,0,target]]);
    await shot('lock-command-before');await save('before');
    game.check('distinct living native accounts begin with no locks',owner!==target&&lists().length===0);
    await chat('/LoCk '+name.toLowerCase());await shot('lock-command-result');
    game.check('typed case-insensitive lock commits exactly one one-way account lock',locked());
    await shot('lock-command-peer',peer);
    await chat('/lock '+name);game.check('duplicate lock is idempotent',locked());
    await chat('/lock');await chat('/lock MissingPlayer');await chat('/lock '+initial.diagnostic.character.name);
    game.check('missing argument unknown target and self target preserve exact lock',locked());
    await shot('lock-command-errors');
    const portal=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0x720);assert(portal);await walk(portal.pos);
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1200});
    game.check('ordinary Vault entry preserves lock without moving peer',locked()&&(await game.observe()).world==='Vault'&&(await peer.observe()).world==='Nexus');
    await game.act({keys:['Insert'],duration_ms:1400});
    game.check('ordinary reconnect retains exact account lock',locked()&&(await game.observe()).world==='Nexus');
    await shot('lock-command-reconnected');await save('reconnected');
    await game.act({keys:[],mouse:{x:645,y:515},duration_ms:300});await shot('lock-command-unlock-menu');
    await game.act({keys:[],mouse:{x:550,y:420},duration_ms:500});
    await shot('lock-command-unlocked');await save('unlocked');
    game.check('existing native Unlock removes the command-created lock',lists().length===0);
    for(const [g,start] of [[game,initial],[peer,other]]){const end=await g.inspect();game.check('actor '+g.actor+' retains original living account character and inventory',end.view.lifecycle.alive&&end.view.session.account===start.view.session.account&&end.view.session.character===start.view.session.character&&JSON.stringify(end.view.character.items)===JSON.stringify(start.view.character.items));}
  }finally{cdp.close();}
}
