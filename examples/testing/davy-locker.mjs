import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:20,seed:863,world:-2,position:'spawn',equipment:[0xb08,0xb24,0xb05,0xac5],backpack:[0xc2e,...Array(7).fill(0xa23)],stats:[670,385,75,25,50,75,40,60]};
const KEYS=[0xe3e,0xe3f,0xe40,0xe41],DOORS=[0x186a,0x186c,0x1868,0x186e],COLORS=['purple','green','red','yellow'];
const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
const routeLength=points=>points.reduce((sum,p,i)=>sum+(i?distance(p,points[i-1]):0),0);
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[],protections=new Map(),participants=[game];
  const guarded=client=>({...client,act:async(...args)=>{for(const p of participants)await protect(p);return client.act(...args);}});game=guarded(game);
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});const v=await r.json();assert(r.ok&&v.status!=='failed',JSON.stringify(v));return v;};
  async function save(name,client=game){const state=await client.inspect();samples.push({name,state});await writeFile(join(out,'davy-observations.json'),JSON.stringify(samples));return state;}
  async function scene(){const v=await game.observe(),r=await fetch(config.origin+'/dev/map?player='+v.session.player,{headers:{Authorization:'Bearer '+token}});assert(r.ok);const m=await r.json();assert(m.width===132&&m.height===132);return m;}
  async function protect(client){const v=await client.observe(),old=protections.get(client.actor);if(old?.player===v.session.player&&v.game_ms<old.until)return;await api({op:'cheat',actor:client.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});protections.set(client.actor,{player:v.session.player,until:v.game_ms+60000});}
  async function add(spec){const p=guarded(await game.join(spec));participants.push(p);await protect(p);return p;}
  function route(m,from,to,mask){
    const blocked=new Set(m.blocked.map(([x,y])=>y*m.width+x));
    for(const e of m.entities){const c=DOORS.findIndex(k=>e.kind===k||e.kind===k+1);if(c>=0&&(mask&(1<<c)))blocked.delete(Math.floor(e.pos.y)*m.width+Math.floor(e.pos.x));}
    for(const e of m.entities){const c=KEYS.indexOf(e.kind);if(c>=0&&!(mask&(1<<c))&&distance(e.pos,to)>1)for(let dy=-1;dy<=1;dy++)for(let dx=-1;dx<=1;dx++)blocked.add((Math.floor(e.pos.y)+dy)*m.width+Math.floor(e.pos.x)+dx);}
    const stride=m.width*2+1,point=n=>({x:(n%stride)/2,y:Math.floor(n/stride)/2});
    const clear=p=>[-.51,0,.51].every(dx=>[-.51,0,.51].every(dy=>p.x+dx>=0&&p.y+dy>=0&&p.x+dx<m.width&&p.y+dy<m.height&&!blocked.has(Math.floor(p.y+dy)*m.width+Math.floor(p.x+dx))));
    const endpoint=p=>{const choices=[];for(let y=Math.round(p.y*2)-3;y<=Math.round(p.y*2)+3;y++)for(let x=Math.round(p.x*2)-3;x<=Math.round(p.x*2)+3;x++){const n=y*stride+x,q=point(n);if(clear(q))choices.push([n,distance(p,q)]);}choices.sort((a,b)=>a[1]-b[1]);assert(choices.length,'No safe endpoint '+JSON.stringify(p));return choices[0][0];};
    const start=endpoint(from),parent=new Map([[start,null]]),queue=[start];
    for(let i=0;i<queue.length;i++){const at=queue[i];for(const next of [at-1,at+1,at-stride,at+stride])if(!parent.has(next)&&clear(point(next))){parent.set(next,at);queue.push(next);}}
    const goals=[...parent.keys()].filter(n=>distance(point(n),to)<1.6).sort((a,b)=>distance(point(a),to)-distance(point(b),to));
    assert(goals.length,'No ordinary route '+JSON.stringify(to));const points=[];for(let at=goals[0];at!==null;at=parent.get(at))points.push(point(at));points.reverse();
    return points.filter((p,i)=>i===0||i===points.length-1||(p.x-points[i-1].x)!==(points[i+1].x-p.x)||(p.y-points[i-1].y)!==(points[i+1].y-p.y));
  }
  async function walk(to,mask,client=game){
    const m=await scene(),from=(await client.observe()).position,points=route(m,from,to,mask);
    await writeFile(join(out,'davy-route-map.json'),JSON.stringify({from,to,mask,map:m,points}));
    for(const p of points)for(let n=0;;n++){
      const v=await client.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y;if(Math.hypot(dx,dy)<.45)break;
      assert(n<50,'Native movement stalled '+JSON.stringify({p,actual:v.position}));await protect(client);
      const keys=Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']);
      const remaining=Math.min(...participants.map(p=>protections.get(p.actor).until))-v.game_ms;
      await client.act({keys,duration_ms:Math.max(30,Math.min(5000,remaining,Math.round(Math.max(Math.abs(dx),Math.abs(dy))/(4+5.6*v.character.stats.speed/75)*1000)))});
    }
  }
  async function reserve(ms){for(let n=0;n<40;n++){for(const p of participants)await protect(p);const now=(await game.observe()).game_ms,remaining=Math.min(...participants.map(p=>protections.get(p.actor).until))-now;if(remaining>=ms)return;await game.act({keys:[],duration_ms:Math.max(50,Math.min(5000,remaining+50))});}throw Error('Fixture protection did not provide a contiguous combat window');}
  async function enter(from,name){await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});for(let i=0;i<8&&(await game.observe()).session.world===from;i++)await game.act({keys:[],duration_ms:500});const s=await save(name);assert.notEqual(s.view.session.world,from);return s;}
  const initial=await save('nexus');await game.act({keys:['1'],duration_ms:500});const entry=await enter(initial.view.session.world,'key-entry');
  game.check('ordinary admission key is consumed and same Wizard enters Locker',entry.view.map.name==="Davy Jones's Locker"&&entry.view.session.character===initial.view.session.character&&entry.view.character.items[4]===-1);
  await protect(game);await game.act({keys:[],duration_ms:800},{name:'locker-entry',when:()=>true});
  const original=await scene(),purple=original.entities.find(e=>e.kind===KEYS[0]);assert(purple);await writeFile(join(out,'davy-initial-map.json'),JSON.stringify(original));
  const peer=await add({...fixture,world:entry.view.session.world,position:{x:purple.pos.x+1.5,y:purple.pos.y},backpack:[-1,...Array(7).fill(0xa23)]});
  await peer.act({keys:['a','w'],duration_ms:120});
  game.check('prepared peer collects natural purple key through native movement',!(await scene()).entities.some(e=>e.id===purple.id));
  await save('peer-purple-discovery',peer);
  // The admission portal lasts one minute; reconnect before traversing the reconstructed maze.
  {
      const browser=config.native[0],cdp=await connect({port:browser.port,target:browser.target,url:config.origin+'/',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
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
      const here=(await game.observe()).position,locked=closed.map(door=>({door,points:route(m,here,door.pos,mask)})).sort((a,b)=>routeLength(a.points)-routeLength(b.points))[0].door;
      await walk(locked.pos,mask);const v=await game.observe(),dx=locked.pos.x-v.position.x,dy=locked.pos.y-v.position.y;
      await game.act({keys:Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']),duration_ms:800},{name:COLORS[color]+'-locked-door',when:()=>true});
      game.check(COLORS[color]+' door rejects actual close contact with only earlier colors',distance((await game.observe()).position,locked.pos)<1.6&&(await scene()).entities.some(e=>e.id===locked.id));
    }
    await walk(key.pos,mask);m=await scene();
    game.check(COLORS[color]+' key location reached through ordinary movement',!m.entities.some(e=>e.id===key.id));mask|=1<<color;
    const v=await game.observe(),choices=m.entities.filter(e=>e.kind===DOORS[color]).map(door=>({door,points:route(m,v.position,door.pos,mask)})).sort((a,b)=>routeLength(a.points)-routeLength(b.points));assert(choices.length);
    const door=choices[0].door;await walk(door.pos,mask);m=await scene();
    game.check(COLORS[color]+' door pair opens on touch with shared key',!m.entities.some(e=>e.id===door.id));
    await save(COLORS[color]+'-peer-state',peer);
    if(color===3)await game.act({keys:[],duration_ms:800},{name:'all-shared-keys',when:()=>true});
  }
  const marker=(await api({op:'world_info',world:entry.view.session.world})).encounter_markers.find(e=>e.kind===0xe32);assert(marker);await walk(marker.position,mask);
  await game.act({keys:[],duration_ms:1500},{name:'locker-captain-reached',when:()=>true});const reached=await save('captain-reached');
  game.check('populated four-color route reveals actual captain on approach',reached.view.lifecycle.alive&&reached.diagnostic.entities.some(e=>e.id===marker.id&&e.stage===1));
  await writeFile(join(out,'davy-map.json'),JSON.stringify(await scene()));
  return {game,peer,walk,scene,mask,route,reserve,add};
}
