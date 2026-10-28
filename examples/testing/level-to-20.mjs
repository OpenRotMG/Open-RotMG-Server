import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';
const root=()=>process.argv[process.argv.indexOf('--run-dir')+1];
const caps=[670,385,75,25,50,75,40,60],growth=[[20,30],[5,15],[1,2],[0,0],[0,2],[1,2],[0,1],[0,2]];
export const fixture={class:0x30e,level:19,seed:777,world:'kitchen',position:{x:13.5,y:40.5},
  equipment:[-1,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:caps.map(n=>n-1)};
export async function enter({actor,account,browser,signal}){
  // Threshold fixtures are not an earned 1–20 journey; mutate only the unloaded disposable character.
  const seeded=execFileSync('python3',['-c',`
import json,sqlite3,sys
with sqlite3.connect(sys.argv[1]) as db:
    row=db.execute('SELECT stats,alive FROM characters WHERE id=? AND account=?',(int(sys.argv[2]),int(sys.argv[3]))).fetchone()
    assert row and row[1]==1
    ch=json.loads(row[0]);assert ch['exp']==50*(ch['level']-1)**2
    if ch['level']<20:
        ch.update(exp=50*ch['level']**2-1,hp=100,mp=200 if ch['level']==19 else 1)
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
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),observations=[],rewards=[];
  const lines=async()=>(await readFile(join(root(),'server.log'),'utf8')).split('\n').slice(0,-1);
  async function awarded(offset,player){
    const deaths=(await lines()).slice(offset).filter(s=>s.startsWith('{')).map(s=>JSON.parse(s).fields).filter(f=>f.message==='enemy defeated'&&f.owner===player);
    assert(deaths.length>0&&deaths.every(d=>[0x6b4,0x926].includes(d.kind)),'Unexpected Kitchen deaths: '+JSON.stringify(deaths));
    const amount=deaths.reduce((sum,d)=>sum+(d.kind===0x926?21:1),0);
    rewards.push({player,amount,deaths});await writeFile(join(root(),'earned-rewards.json'),JSON.stringify(rewards,null,2));return amount;
  }
  async function protect(client){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:client.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function snapshot(name,client=game){const state=await client.inspect();observations.push({name,state});await writeFile(join(root(),'level-observations.json'),JSON.stringify({preparedThresholdsAndGear:true,observations},null,2));return state;}
  async function earn(client){
    const before=await client.inspect(),boss=before.diagnostic.entities.find(e=>e.kind===0x926);assert(boss);
    const offset=(await lines()).length;
    for(let n=0;n<4;n++){
      await client.act({keys:[],duration_ms:150,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:false}});
      await client.act({keys:[' '],duration_ms:150});
      await client.act({keys:[],duration_ms:2000});
      if((await client.inspect()).view.character.experience>before.view.character.experience)return offset;
    }
    assert.fail('Native spell did not earn XP');
  }
  await protect(game);
  const start=await snapshot('prepared-19');
  const peer=await game.join({...fixture,level:1,world:start.view.session.world,position:{x:start.view.position.x,y:start.view.position.y-1},stats:null});
  await protect(peer);
  await game.act({keys:[],duration_ms:1800,mouse:{x:700,y:240}},{name:'level-19-before',when:()=>true});
  const before=await snapshot('before-kill'),peerBefore=await snapshot('peer-before-kill',peer);
  game.check('honest near-threshold fixtures loaded',before.view.character.level===19&&before.view.character.experience===18049&&peerBefore.view.character.level===1&&peerBefore.view.character.experience===49);
  game.check('both resources are depleted before level-up',before.view.character.hp<before.view.character.stats[0]&&before.view.character.mp<before.view.character.stats[1]);
  const offset=await earn(game);
  await game.act({keys:[],duration_ms:800},{name:'level-20-after',when:()=>true});
  const after=await snapshot('after-kill'),peerAfter=await snapshot('peer-after-kill',peer);
  const award=await awarded(offset,before.view.session.player);
  for(const [label,a,b] of [['finishing level',before,after],['shared first level',peerBefore,peerAfter]]){
    const x=a.view.character,y=b.view.character;
    game.check(label+' receives attributed real XP and one level',y.experience-x.experience===award&&y.level===x.level+1);
    // Inclusive growth and resource refill: selected Wizard XML and pinned Kithio CheckLevelUp, https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.Leveling.cs#L194-L220
    game.check(label+' growth stays inside selected ranges/caps',y.base_stats.every((v,i)=>v>=Math.min(caps[i],x.base_stats[i]+growth[i][0])&&v<=Math.min(caps[i],x.base_stats[i]+growth[i][1])));
    game.check(label+' refills HP and MP',y.hp===y.stats[0]&&y.mp===y.stats[1]);
    game.check(label+' remains the same living character',b.view.lifecycle.alive&&b.view.session.character===a.view.session.character);
  }
  game.check('nearby nonattacking peer gains XP without damage credit',peerAfter.diagnostic.character.shots===0&&peerAfter.diagnostic.character.hits===0);
  await peer.act({keys:[],duration_ms:1000,mouse:{x:700,y:240}},{name:'shared-level-2',when:()=>true});
  await game.act({keys:['Insert'],duration_ms:500});
  const retained=await snapshot('nexus-reconnect');
  game.check('ordinary Nexus reconnect persists level XP and growth',retained.view.map.name==='Nexus'&&retained.view.session.character===after.view.session.character&&JSON.stringify(retained.view.character.base_stats)===JSON.stringify(after.view.character.base_stats)&&retained.view.character.experience===after.view.character.experience&&retained.view.character.level===20);
  const capped=await game.join({...fixture,level:20});
  await protect(capped);
  const capBefore=await snapshot('cap-before',capped),capOffset=await earn(capped);
  const capAfter=await snapshot('cap-after',capped),capAward=await awarded(capOffset,capBefore.view.session.player);
  game.check('level 20 earns further XP without level or stat growth',capAfter.view.character.experience-capBefore.view.character.experience===capAward&&capAfter.view.character.level===20&&JSON.stringify(capAfter.view.character.base_stats)===JSON.stringify(capBefore.view.character.base_stats));
  const notices=(await lines()).filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields).filter(f=>f.message==='level twenty announced');
  game.check('committed level20 announces once to both current-world players, not on lower levels reconnect or capped XP',notices.length===1&&notices[0].player===before.view.session.player&&notices[0].recipients===2);
}
