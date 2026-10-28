import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {setTimeout as wait} from 'node:timers/promises';
export {enter} from './level-to-20.mjs';
const root=()=>process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x30e,level:19,seed:889,world:'kitchen',position:{x:13.5,y:40.5},
  equipment:[-1,0xa2e,-1,-1],backpack:[0xc42,0xc42,-1,-1,-1,-1,-1,-1],stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root(),'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));return receipt.result;
  }
  async function snapshot(name,client=game){const state=await client.inspect();samples.push({name,state});await writeFile(join(root(),'booster-observations.json'),JSON.stringify(samples,null,2));return state;}
  await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});
  await game.act({keys:[],duration_ms:1800});
  const start=(await game.observe()).game_ms;
  await game.act({keys:[],duration_ms:800},{name:'booster-before',when:v=>v.game_ms>=start+400});
  const before=await snapshot('before');
  game.check('prepared threshold is below level 20',before.view.character.experience===18049&&before.view.character.level===19);
  await game.act({keys:['1'],duration_ms:300});
  const used=(await game.observe()).game_ms;
  await game.act({keys:[],duration_ms:800},{name:'booster-active',when:v=>v.game_ms>=used+400});
  const active=await snapshot('active');
  game.check('ordinary booster activates with exactly one inventory commit',active.diagnostic.character.xp_boost.active&&active.view.character.items[4]===-1&&active.view.character.inventory_revision===before.view.character.inventory_revision+1);
  await game.act({keys:['2'],duration_ms:300});const refused=await snapshot('duplicate');
  game.check('duplicate refuses without changing inventory or boost',JSON.stringify(refused.view.character.items)===JSON.stringify(active.view.character.items)&&refused.view.character.inventory_revision===active.view.character.inventory_revision&&JSON.stringify(refused.diagnostic.character.xp_boost)===JSON.stringify(active.diagnostic.character.xp_boost));
  const boss=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0x926);assert(boss);
  const lines=async()=>(await readFile(join(root(),'server.log'),'utf8')).split('\n').slice(0,-1);
  const offset=(await lines()).length;
  for(let n=0;n<4;n++){
    await game.act({keys:[],duration_ms:150,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:false}});
    await game.act({keys:[' '],duration_ms:150});await game.act({keys:[],duration_ms:2000});
    if((await game.observe()).character.level===20)break;
  }
  await game.act({keys:[],duration_ms:800},{name:'booster-expired',when:v=>v.character.level===20});
  const crossed=await snapshot('crossed'),ch=crossed.diagnostic.character;
  const events=(await lines()).slice(offset).filter(s=>s.startsWith('{')).map(s=>JSON.parse(s).fields);
  const kills=events.filter(f=>f.message==='enemy defeated'&&f.owner===before.view.session.player);
  assert(kills.length&&kills.every(k=>[0x6b4,0x926].includes(k.kind)),JSON.stringify(kills));
  const earned=kills.reduce((sum,k)=>sum+(k.kind===0x926?21:1),0);
  const commits=events.filter(f=>f.message==='progress committed'&&f.player===before.view.session.player&&f.experience>18049),first=commits[0];assert(first);
  await writeFile(join(root(),'booster-kills.json'),JSON.stringify({kills,earned,commits},null,2));
  // The selected item promises a lifetime, not last-kill rounding; whole-award doubling is reconstruction: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf
  game.check('native defeat doubles the crossing award but not later awards',ch.level===20&&!ch.xp_boost.active&&first.bonus_experience>0&&first.experience-18049===2*first.bonus_experience&&ch.xp_boost.bonus_exp===first.bonus_experience&&ch.exp-18049===earned+first.bonus_experience);
  game.check('crossing bonus never contributes to Fame',ch.fame===Math.floor((ch.exp-ch.xp_boost.bonus_exp)/1000));
  await game.act({keys:['2'],duration_ms:300});const capped=await snapshot('level-20-refusal');
  game.check('expired level-20 character cannot consume another booster',capped.view.character.items[5]===0xc42&&capped.view.character.inventory_revision===crossed.view.character.inventory_revision&&!capped.diagnostic.character.xp_boost.active);
  await game.act({keys:['Insert'],duration_ms:800});const nexus=await snapshot('expired-reconnect');
  game.check('Nexus reconnect retains expired boost and earned progress',nexus.view.map.name==='Nexus'&&nexus.view.session.character===crossed.view.session.character&&nexus.view.session.player!==crossed.view.session.player&&nexus.diagnostic.character.exp===ch.exp&&JSON.stringify(nexus.diagnostic.character.xp_boost)===JSON.stringify(ch.xp_boost));
  const living=await game.join({...fixture,level:1});
  await api({op:'cheat',actor:living.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});
  await living.act({keys:['1'],duration_ms:300});const live=await snapshot('active-before-reconnect',living);
  await living.act({keys:['Insert'],duration_ms:800});const retained=await snapshot('active-reconnect',living);
  game.check('active boost survives same-character native reconnect',live.diagnostic.character.xp_boost.active&&retained.view.map.name==='Nexus'&&retained.view.session.player!==live.view.session.player&&retained.view.session.character===live.view.session.character&&retained.view.character.items[4]===-1&&retained.diagnostic.character.xp_boost.active);
  const {world}=await api({op:'world',template:'abyss-of-demons'}),info=await api({op:'world_info',world});assert(info.damaging_ground.length);
  const position=info.damaging_ground[0];
  const victim=await api({op:'spawn',spec:{...fixture,level:1,world,position,equipment:[-1,-1,-1,-1],stats:null,guid:'booster-death',controller:'direct',profile:null}});
  await api({op:'cheat',actor:victim.actor,change:{kind:'protection',condition:'invincible',duration_ms:5000}});
  await Promise.all([game.act({keys:[],duration_ms:1800}),(async()=>{
    const end=Date.now()+10000;
    while((await game.observe()).paused){assert(Date.now()<end,'Native observer did not resume');await wait(30);}
    await api({op:'act',actor:victim.actor,action:{command:'use',slot:4,x:position.x,y:position.y},duration_ms:0,pauseAfter:false});
  })()]);
  const doomed=await snapshot('direct-active-before-hazard-death',{inspect:()=>api({op:'inspect',actor:victim.actor})});
  game.check('direct lifecycle companion uses ordinary booster handler before real hazard damage',doomed.diagnostic.character.xp_boost.active&&doomed.view.lifecycle.alive);
  await game.act({keys:[],duration_ms:8000});
  const dead=JSON.parse(execFileSync('python3',['-c',`import json,sqlite3,sys
with sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True) as db:
 row=db.execute('SELECT alive,stats,items FROM characters WHERE id=?',(int(sys.argv[2]),)).fetchone()
 print(json.dumps({'alive':row[0],'character':json.loads(row[1]),'items':json.loads(row[2]),'deaths':db.execute('SELECT count(*) FROM deaths WHERE character=?',(int(sys.argv[2]),)).fetchone()[0]}))`,join(config.state,'game.db'),String(doomed.view.session.character)],{encoding:'utf8'}));
  await writeFile(join(root(),'booster-death.json'),JSON.stringify(dead,null,2));
  game.check('real hazard death commits once and clears active boost',dead.alive===0&&dead.deaths===1&&!dead.character.xp_boost.active&&dead.items[4]===-1);
}
