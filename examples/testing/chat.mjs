import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';

assert(process.env.CHAT_WORLD===undefined||process.env.CHAT_WORLD==='dungeon','CHAT_WORLD must be unset (Nexus) or dungeon');
const dungeon=process.env.CHAT_WORLD==='dungeon';
export const fixture={class:0x30e,level:20,seed:904,world:dungeon?'sprite-world':-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
export async function run(game){
  const config=JSON.parse(await readFile(join(root(),'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const api=async command=>{
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});
    const value=await response.json();assert(response.ok&&value.status==='succeeded',JSON.stringify(value));return value;
  };
  const initial=await game.inspect(),position=initial.view.position;
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});
  assert(response.ok);const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  let far;
  for(let y=1;y<map.height-1&&!far;y++)for(let x=1;x<map.width-1;x++){
    const distance=Math.hypot(x+.5-position.x,y+.5-position.y);
    if(distance>20&&distance<23&&!blocked.has([x,y].join(','))){far={x:x+.5,y:y+.5};break;}
  }
  assert(far,'A walkable listener position beyond Nexus proximity is required');
  const peer=await game.join({...fixture,world:initial.view.session.world,position:far,seed:905});
  if(dungeon)for(const actor of [game.actor,peer.actor])await api({op:'cheat',actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});
  const native=JSON.parse(await readFile(join(root(),'native.json'),'utf8')),pages=[];
  const observations=[];
  try{
    for(const actor of [game.actor,peer.actor]){
      const browser=native.native.find(n=>n.actor===actor);assert(browser);
      pages.push(await connect({...browser,url:config.origin+'/',helper:config.browserTools}));
    }
    async function key(page,key,code,vk){for(const type of ['keyDown','keyUp'])await page.call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode:vk});}
    async function say(index,text,viewer,name,recipients){
      const before=(await game.observe()).game_ms;
      const action=viewer.act({keys:[],duration_ms:2800},{name,when:v=>v.game_ms>=before+1900});
      const input=(async()=>{
        const deadline=Date.now()+10000;while((await game.observe()).paused){assert(Date.now()<deadline,'Native action did not unpause');await delay(20);}
        if(typeof text==='string'){
          await key(pages[index],'Enter','Enter',13);if(text.length)await pages[index].call('Input.insertText',{text});await key(pages[index],'Enter','Enter',13);
        }else await key(pages[index],...text);
      })();
      const results=await Promise.allSettled([action,input]);for(const r of results)if(r.status==='rejected')throw r.reason;
      const events=(await readFile(join(root(),'server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l));
      const published=events.filter(e=>e.fields?.message==='public chat delivered'),delivered=published.at(-1);
      if(recipients!==undefined)game.check(name+' server audience',delivered?.fields.recipients===recipients);
      if(text==='')game.check('empty chat closes without publication',published.length===observations.at(-1).published);
      observations.push({name,text,published:published.length,delivered,speaker:await game.inspect(),listener:await peer.inspect()});
      await writeFile(join(root(),'chat-observations.json'),JSON.stringify(observations,null,2));
    }
    game.check('two distinct living native accounts start over twenty tiles apart',initial.view.session.account!==(await peer.inspect()).view.session.account&&Math.hypot(far.x-position.x,far.y-position.y)>20);
    await say(0,'distant904',peer,'distant-listener',dungeon?2:1);
    await game.act({keys:[],duration_ms:600},{name:'distant-speaker',when:()=>true});
    await say(0,'/yell yell904',peer,'yell-listener',dungeon?undefined:2);
    await game.act({keys:[],duration_ms:600},{name:'yell-speaker',when:()=>true});
    if(dungeon){
      await say(0,'/who',game,'dungeon-who');
      await say(0,['Insert','Insert',45],game,'nexus-escape');
      game.check('ordinary Insert escape transfers only speaker to Nexus',(await game.inspect()).view.session.world===-2&&(await peer.inspect()).view.session.world===initial.view.session.world);
      await say(0,'separate904',peer,'different-world-listener',1);
      await game.act({keys:[],duration_ms:600},{name:'different-world-speaker',when:()=>true});
    }else{
      const name=initial.diagnostic.character.name;
      await say(1,'/teleport '+name,peer,'teleport-near');
      game.check('ordinary native teleport makes listener nearby',Math.hypot((await peer.observe()).position.x-position.x,(await peer.observe()).position.y-position.y)<.2);
      await say(0,'near904',peer,'near-listener',2);
      await game.act({keys:[],duration_ms:600},{name:'near-speaker',when:()=>true});
      await say(0,'/help',game,'help');
      await say(0,'/who',game,'who');
      await say(0,'/who extra',game,'who-refused');
      await say(0,'/unknown904',game,'unknown-refused');
      await say(0,'',game,'empty-cancelled');
      await say(0,['PageUp','PageUp',33],game,'history-up');
      await say(0,['PageDown','PageDown',34],game,'history-down');
    }
    game.check('chat and refusals preserve both living character identities',(await game.inspect()).view.session.character===initial.view.session.character&&(await game.observe()).lifecycle.alive&&(await peer.observe()).lifecycle.alive);
  }finally{for(const page of pages)page.close();}
}
