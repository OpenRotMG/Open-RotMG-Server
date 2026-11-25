import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';
import {route as nativeRoute,walk as nativeWalk} from './native-route.mjs';

export const fixture={class:0x30e,level:20,seed:863,world:-2,position:'spawn',equipment:[0xb08,0xb24,0xb05,0xac5],backpack:[0xc2e,...Array(7).fill(0xa23)],stats:[670,385,75,25,50,75,40,60]};
const BARREL=0x182e,KEYS=[0xe3e,0xe3f,0xe40,0xe41],DOORS=[0x186a,0x186c,0x1868,0x186e],COLORS=['purple','green','red','yellow'];
const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
const routeLength=points=>points.reduce((sum,p,i)=>sum+(i?distance(p,points[i-1]):0),0);
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[],protections=new Map(),participants=[game];
  const guarded=client=>({...client,act:async(...args)=>{for(const p of participants)await protect(p);return client.act(...args);}});game=guarded(game);
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});const v=await r.json();assert(r.ok&&v.status!=='failed',JSON.stringify(v));return v;};
  async function save(name,client=game){const state=await client.inspect();samples.push({name,state});await writeFile(join(out,'davy-observations.json'),JSON.stringify(samples));return state;}
  async function scene(client=game){const v=await client.observe(),r=await fetch(config.origin+'/dev/map?player='+v.session.player,{headers:{Authorization:'Bearer '+token}});assert(r.ok);const m=await r.json();assert(m.width===256&&m.height===256);return m;}
  async function protect(client){const v=await client.observe(),old=protections.get(client.actor);if(old?.player===v.session.player&&v.game_ms<old.until)return;await api({op:'cheat',actor:client.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});protections.set(client.actor,{player:v.session.player,until:v.game_ms+60000});}
  async function add(spec){const p=guarded(await game.join(spec));participants.push(p);await protect(p);return p;}
  // Restored key rooms are lined with destructible GhostShip Barrels; held doors open only for shared key colours.
  function plan(m,to,mask){
    const W=m.width,tile=p=>Math.floor(p.y)*W+Math.floor(p.x),open=[],avoid=[];
    for(const e of m.entities){const c=DOORS.findIndex(k=>e.kind===k||e.kind===k+1);if(c>=0&&(mask&(1<<c)))open.push(tile(e.pos));}
    for(const e of m.entities){const c=KEYS.indexOf(e.kind);if(c>=0&&!(mask&(1<<c))&&distance(e.pos,to)>1)for(let dy=-1;dy<=1;dy++)for(let dx=-1;dx<=1;dx++)avoid.push((Math.floor(e.pos.y)+dy)*W+Math.floor(e.pos.x)+dx);}
    return {open,avoid,soft:new Map(m.entities.filter(e=>e.kind===BARREL).map(e=>[tile(e.pos),e]))};
  }
  const route=(m,from,to,mask)=>nativeRoute(m,from,to,plan(m,to,mask));
  function reachableDoor(m,from,doors,mask){
    // A paired door can be visible on the map but reachable only from its still-locked side.
    const reachable=doors.flatMap(door=>{
      try{return [{door,points:route(m,from,door.pos,mask).points}];}
      catch(error){if(error.code!=='ERR_ASSERTION'||!error.message.startsWith('No ordinary route '))throw error;return [];}
    });
    assert(reachable.length,'No reachable approach to generated door pair');
    return reachable.sort((a,b)=>routeLength(a.points)-routeLength(b.points))[0].door;
  }
  const walk=(to,mask,client=game)=>nativeWalk({client,map:scene,to,options:m=>plan(m,to,mask),record:r=>writeFile(join(out,'davy-route-map.json'),JSON.stringify({...r,mask})),
    ready:async()=>{for(const p of participants)await protect(p);return Math.min(...participants.map(p=>protections.get(p.actor).until))-(await client.observe()).game_ms;}});
  async function reserve(ms){for(let n=0;n<40;n++){for(const p of participants)await protect(p);const now=(await game.observe()).game_ms,remaining=Math.min(...participants.map(p=>protections.get(p.actor).until))-now;if(remaining>=ms)return;await game.act({keys:[],duration_ms:Math.max(50,Math.min(5000,remaining+50))});}throw Error('Fixture protection did not provide a contiguous combat window');}
  async function enter(from,name){await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});for(let i=0;i<8&&(await game.observe()).session.world===from;i++)await game.act({keys:[],duration_ms:500});const s=await save(name);assert.notEqual(s.view.session.world,from);return s;}
  const initial=await save('nexus');await game.act({keys:['1'],duration_ms:500});const entry=await enter(initial.view.session.world,'key-entry');
  game.check('ordinary admission key is consumed and same Wizard enters Locker',entry.view.map.name==="Davy Jones's Locker"&&entry.view.session.character===initial.view.session.character&&entry.view.character.items[4]===-1);
  await protect(game);await game.act({keys:[],duration_ms:800},{name:'locker-entry',when:()=>true});
  const original=await scene(),purple=original.entities.find(e=>e.kind===KEYS[0]);assert(purple);await writeFile(join(out,'davy-initial-map.json'),JSON.stringify(original));
  const peer=await add({...fixture,world:entry.view.session.world,position:{x:purple.pos.x+1.5,y:purple.pos.y},backpack:[-1,...Array(7).fill(0xa23)]});
  await peer.act({keys:['a','w'],duration_ms:120});
  for(let n=0;n<4&&(await scene()).entities.some(e=>e.id===purple.id);n++)await peer.act({keys:[],duration_ms:250});
  game.check('prepared peer collects natural purple key through native movement',!(await scene()).entities.some(e=>e.id===purple.id));
  await save('peer-purple-discovery',peer);
  // The admission portal lasts one minute; reconnect before traversing the reconstructed layout.
  {
      const browser=config.native[0],cdp=await connect({port:browser.port,target:browser.target,owner:browser.owner,url:config.origin+'/',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
      try{
        assert((await game.observe()).paused,'Reconnect setup requires the runner pause boundary');
        const previous=await cdp.eval('performance.timeOrigin'),deadline=Date.now()+30000;await cdp.call('Page.reload',{});
        for(;;){assert(Date.now()<deadline,'Reload did not start fresh Ruffle');if(await cdp.eval('performance.timeOrigin!=='+previous+'&&document.querySelector("ruffle-player")?.ruffle().isPlaying===true'))break;await new Promise(ok=>setTimeout(ok,50));}
        await cdp.eval('(()=>{const p=document.querySelector("ruffle-player"),m=p.shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
        await cdp.call('Emulation.setFocusEmulationEnabled',{enabled:true});
        for(;;){
          const state=await api({op:'native_state',actors:[game.actor,peer.actor]});if(state.actors.find(a=>a.actor===game.actor)?.stage==='ready')break;
          assert(Date.now()<deadline,'Normal reconnect did not load the same character');
          const r=await cdp.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y};})()'),p={x:r.x+400,y:r.y+550,button:'left',clickCount:1};
          await cdp.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});await cdp.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});await new Promise(ok=>setTimeout(ok,250));
        }
      }finally{cdp.close();}
      await game.act({keys:[],duration_ms:500});
      const reloaded=await save('ordinary-reconnect');
      game.check('ordinary reload preserves same living character through fresh guest Tutorial routing',reloaded.view.lifecycle.alive&&reloaded.view.session.character===initial.view.session.character&&reloaded.view.map.name==='Tutorial');
      await game.act({keys:['Insert'],duration_ms:700});
      for(let n=0;n<8&&(await game.observe()).world!=='Nexus';n++)await game.act({keys:[],duration_ms:500});
      const nexus=await save('tutorial-exit-after-reload');assert.equal(nexus.view.map.name,'Nexus');
      const admission=nexus.view.objects.find(e=>e.kind===0x741);assert(admission,'Original admission portal expired before reconnect');
      for(let n=0;distance((await game.observe()).position,admission.position)>.65;n++){assert(n<30);const v=await game.observe(),dx=admission.position.x-v.position.x,dy=admission.position.y-v.position.y;await game.act({keys:Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']),duration_ms:100});}
      const reentered=await enter(nexus.view.session.world,'reentered-shared-locker');
      game.check('same character reenters original shared instance after disconnect',reentered.view.session.world===entry.view.session.world);
      game.check('reconnect preserves shared purple discovery',!(await scene()).entities.some(e=>e.kind===KEYS[0]));
      await protect(game);
      await game.act({keys:[],duration_ms:900},{name:'reconnected-purple-key-hud',when:()=>true});
  }
  let mask=1;
  // Shared touch-open keys are documented independently of the lost generator: https://wikiwiki.jp/rmd/Davy%20Jones%27s%20Locker
  for(let color=0;color<4;color++){
    let m=await scene();const key=original.entities.find(e=>e.kind===KEYS[color]);assert(key);
    const closed=m.entities.filter(e=>e.kind===DOORS[color]);assert(closed.length);
    if(color===1){
      const here=(await game.observe()).position,locked=reachableDoor(m,here,closed,mask);
      await walk(locked.pos,mask);const v=await game.observe(),dx=locked.pos.x-v.position.x,dy=locked.pos.y-v.position.y;
      await game.act({keys:Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']),duration_ms:800},{name:COLORS[color]+'-locked-door',when:()=>true});
      game.check(COLORS[color]+' door rejects actual close contact with only earlier colors',distance((await game.observe()).position,locked.pos)<1.6&&(await scene()).entities.some(e=>e.id===locked.id));
    }
    await walk(key.pos,mask);
    // The recovered key retires itself on its next tick once a player is within 1 tile (actors.xml:11951).
    for(let n=0;n<4&&(await scene()).entities.some(e=>e.id===key.id);n++)await game.act({keys:[],duration_ms:250});
    m=await scene();
    game.check(COLORS[color]+' key location reached through ordinary movement',!m.entities.some(e=>e.id===key.id));mask|=1<<color;
    const v=await game.observe(),door=reachableDoor(m,v.position,m.entities.filter(e=>e.kind===DOORS[color]),mask);await walk(door.pos,mask);m=await scene();
    game.check(COLORS[color]+' door pair opens on touch with shared key',!m.entities.some(e=>e.id===door.id));
    await save(COLORS[color]+'-peer-state',peer);
    if(color===3)await game.act({keys:[],duration_ms:800},{name:'all-shared-keys',when:()=>true});
  }
  const marker=(await api({op:'world_info',world:entry.view.session.world})).encounter_markers.find(e=>e.kind===0xe32);assert(marker);await walk(marker.position,mask);
  await game.act({keys:[],duration_ms:1500},{name:'locker-captain-reached',when:()=>true});const reached=await save('captain-reached');
  // Recovered Davy Jones (actors.xml:11145) leaves its top-level invincible Ini state once a player is within 8 tiles.
  const captainStates=(reached.diagnostic.xml[marker.id]?.states??[]).map(x=>x.name);
  game.check('populated four-color route wakes actual captain on approach',reached.view.lifecycle.alive&&reached.diagnostic.entities.some(e=>e.id===marker.id)&&captainStates.length>1&&captainStates[1]!=='Ini');
  await writeFile(join(out,'davy-map.json'),JSON.stringify(await scene()));
  return {game,peer,walk,scene,mask,route,reserve,add};
}
