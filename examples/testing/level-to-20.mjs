import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';
const root=()=>process.argv[process.argv.indexOf('--run-dir')+1];
const caps=[670,385,75,25,50,75,40,60],growth=[[20,30],[5,15],[1,2],[0,0],[0,2],[1,2],[0,1],[0,2]];
export const fixture={class:0x30e,level:19,seed:777,world:'sprite-world',worldSeed:1,
  position:world=>{const boss=world.encounter_markers.find(e=>e.kind===0xd06);assert(boss);return{x:boss.position.x-4,y:boss.position.y};},
  equipment:[0xb08,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:caps.map(n=>n-1)};
export async function enter({actor,account,browser,signal}){
  // Threshold fixtures are not an earned 1–20 journey; mutate only the unloaded disposable character.
  const seeded=execFileSync('python3',['-c',`
import json,sqlite3,sys
with sqlite3.connect(sys.argv[1]) as db:
    row=db.execute('SELECT stats,alive FROM characters WHERE id=? AND account=?',(int(sys.argv[2]),int(sys.argv[3]))).fetchone()
    assert row and row[1]==1
    ch=json.loads(row[0]);assert ch['exp']==50*(ch['level']-1)**2
    if ch['level']<20:
        ch.update(exp=50*ch['level']**2-1,hp=100,mp=1)
        ch['fame']=ch['exp']//1000
        assert db.execute('UPDATE characters SET stats=? WHERE id=? AND account=?',(json.dumps(ch),int(sys.argv[2]),int(sys.argv[3]))).rowcount==1
    print(json.dumps({'actor':int(sys.argv[2]),'account':int(sys.argv[3]),'character':ch,'prepared':True}))
`,join(root(),'state','game.db'),String(actor),String(account)],{encoding:'utf8',signal});
  await writeFile(join(root(),'threshold-'+actor+'.json'),seeded);
  const pages=await (await fetch(`http://127.0.0.1:${browser.port}/json/list`,{signal})).json();
  const target=pages.find(p=>p.id===browser.target);assert(target);
  const page=await connect({...browser,url:target.url,helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  try{
    await page.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
    const r=await page.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
    assert.equal(r.width,800);assert.equal(r.height,600);
    await page.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    for(let n=0;n<2;n++){
      const p={x:r.x+400,y:r.y+550,button:'left',clickCount:1};
      try{await page.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}
      finally{await page.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
      await page.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
    }
  }finally{page.close();}
}
export async function run(game){
  const config=JSON.parse(await readFile(join(root(),'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),observations=[];
  async function protect(client){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:client.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function snapshot(name,client=game){const state=await client.inspect();observations.push({name,state});await writeFile(join(root(),'level-observations.json'),JSON.stringify({preparedThresholdsAndGear:true,observations},null,2));return state;}
  async function defeat(client){
    const boss=(await client.inspect()).diagnostic.entities.find(e=>e.kind===0xd06);assert(boss);
    for(let n=0;n<35;n++){
      if(!(await client.inspect()).diagnostic.entities.some(e=>e.id===boss.id))return;
      await client.act({keys:[],duration_ms:800,target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:true}});
    }
    assert.fail('Native staff did not defeat Limon');
  }
  await protect(game);
  const start=await snapshot('prepared-19');
  const {worldSeed,...shared}=fixture;
  const peer=await game.join({...shared,level:1,world:start.view.session.world,position:{x:start.view.position.x,y:start.view.position.y-1},equipment:[-1,0xa2e,-1,-1],stats:null});
  await protect(peer);
  await game.act({keys:[],duration_ms:1800},{name:'level-19-before',when:()=>true});
  const before=await snapshot('before-kill'),peerBefore=await snapshot('peer-before-kill',peer);
  game.check('honest near-threshold fixtures loaded',before.view.character.level===19&&before.view.character.experience===18049&&peerBefore.view.character.level===1&&peerBefore.view.character.experience===49);
  game.check('both resources are depleted before level-up',before.view.character.hp<before.view.character.stats.max_hp&&before.view.character.mp<before.view.character.stats.max_mp);
  await defeat(game);
  await game.act({keys:[],duration_ms:800},{name:'level-20-after',when:()=>true});
  const after=await snapshot('after-kill'),peerAfter=await snapshot('peer-after-kill',peer);
  for(const [label,a,b,award] of [['finishing level',before,after,630],['shared first level',peerBefore,peerAfter,25]]){
    const x=a.view.character,y=b.view.character;
    game.check(label+' receives capped real XP and one level',y.experience-x.experience===award&&y.level===x.level+1);
    // Inclusive growth and resource refill: selected Wizard XML and pinned Kithio CheckLevelUp, https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.Leveling.cs#L194-L220
    game.check(label+' growth stays inside selected ranges/caps',y.base_stats.every((v,i)=>v>=Math.min(caps[i],x.base_stats[i]+growth[i][0])&&v<=Math.min(caps[i],x.base_stats[i]+growth[i][1])));
    game.check(label+' refills HP and MP',y.hp===y.stats.max_hp&&y.mp===y.stats.max_mp);
    game.check(label+' remains the same living character',b.view.lifecycle.alive&&b.view.session.character===a.view.session.character);
  }
  game.check('nearby nonattacking peer gains XP without damage credit',peerAfter.diagnostic.character.shots===0&&peerAfter.diagnostic.character.hits===0);
  await peer.act({keys:[],duration_ms:1000},{name:'shared-level-2',when:()=>true});
  await game.act({keys:['Insert'],duration_ms:500});
  const retained=await snapshot('nexus-reconnect');
  game.check('ordinary Nexus reconnect persists level XP and growth',retained.view.world==='Nexus'&&retained.view.session.character===after.view.session.character&&JSON.stringify(retained.view.character.base_stats)===JSON.stringify(after.view.character.base_stats)&&retained.view.character.experience===after.view.character.experience&&retained.view.character.level===20);
  const capped=await game.join({...fixture,level:20,stats:caps});
  await protect(capped);
  const capBefore=await snapshot('cap-before',capped);
  await defeat(capped);
  const capAfter=await snapshot('cap-after',capped);
  game.check('level 20 earns further XP without level or stat growth',capAfter.view.character.experience-capBefore.view.character.experience===630&&capAfter.view.character.level===20&&JSON.stringify(capAfter.view.character.base_stats)===JSON.stringify(caps));
}
