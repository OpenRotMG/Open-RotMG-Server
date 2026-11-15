import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const spec={class:0x310,level:4,seed:813,world:1,equipment:[0xa04,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,25,0,50,25,40,60]};
let mage;
export async function prepare(api){
  const tools=[];
  for(const [path,expected] of [[join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),'2bb207a0c296610efc1646ae121360af81bdfa73589a4945c37829d3ad97fd28'],[resolve('scripts/game.mjs'),'c1c17470c8a992af88428b39e0b6eee0ee298248e72dec9b002bc6d6c8ecbbbb']]){
    const sha256=createHash('sha256').update(await readFile(path)).digest('hex');assert.equal(sha256,expected);tools.push({path,sha256});
  }
  await writeFile(join(process.env.QA_RESULTS_DIR,'native-tools.json'),JSON.stringify(tools));
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'goblin-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const root=join(process.env.QA_RESULTS_DIR,'native');
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  mage=map.entities.find(e=>e.kind===0x614&&Array.from({length:25},(_,i)=>!blocked.has([Math.floor(e.pos.x-10+i*.5),Math.floor(e.pos.y)].join(','))).every(Boolean));
  assert(mage,'No natural Goblin Mage with clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify(map.entities.filter(e=>e.camp===mage.camp)));
  return {...spec,position:{x:mage.pos.x-9.5,y:mage.pos.y}};
}
export async function run(game){
  let serial=0,protectedUntil=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===mage.camp&&[0x612,0x613,0x614].includes(e.kind));
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'goblin-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);protectedUntil=now+60000;
  }
  const act=async(input,capture)=>{await protect();while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await game.act({keys:[],duration_ms:250});await protect();}await game.act(input,capture);};
  const begin=await save('arrival');
  assert(family(begin).some(e=>e.id===mage.id&&e.hp===280));
  await act({keys:['d'],duration_ms:600});
  const combat=[],bags=s=>s.diagnostic.entities.filter(e=>[0x500,0x503,0x507,0x508,0x509].includes(e.kind));
  let lastAlive,priorBags;
  for(let n=0;n<45;n++){
    const s=await game.inspect(),e=family(s).find(e=>e.id===mage.id);if(!e)break;
    lastAlive=e;priorBags=new Set(bags(s).map(b=>b.id));
    const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,a=-Math.PI/4;
    const x=dx*Math.cos(a)+dy*Math.sin(a),y=-dx*Math.sin(a)+dy*Math.cos(a),length=Math.hypot(x,y);assert(length>0);
    // A short aim ray avoids the chat panel when the retreat projects near the canvas edge.
    await act({keys:[],duration_ms:250,mouse:{x:300+140*x/length,y:325+140*y/length}});
    await act({keys:[],duration_ms:650});combat.push(await game.inspect());
  }
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify(combat));
  await act({keys:[],duration_ms:1000});
  const dead=await save('mage-defeated'),remaining=family(dead);
  game.check('native wand defeats the natural Mage and awards XP',!family(dead).some(e=>e.id===mage.id)&&dead.view.character.experience>begin.view.character.experience);
  game.check('ordinary Mage defeat leaves both escort roles alive',[0x612,0x613].every(kind=>remaining.some(e=>e.kind===kind)));
  await act({keys:[],duration_ms:700},{name:'goblin-defeated',when:()=>true});
  const drops=bags(dead).filter(b=>!priorBags.has(b.id)&&Math.hypot(b.pos.x-lastAlive.pos.x,b.pos.y-lastAlive.pos.y)<1.5&&b.items[0]>=0);
  await writeFile(join(process.env.QA_RESULTS_DIR,'mage-drops.json'),JSON.stringify({lastAlive,priorBagIds:[...priorBags],drops}));
  assert(drops.length>0,'Mage rolled no attributable bag; pickup/persistence remain untested');
  const bag=drops[0],item=bag.items[0];assert.equal(bag.bag_owners.length,0);
  for(let n=0;n<60;n++){
    const v=await game.observe(),dx=bag.pos.x-v.position.x,dy=bag.pos.y-v.position.y,distance=Math.hypot(dx,dy);if(distance<.35)break;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
    await act({keys,duration_ms:Math.max(50,Math.min(200,Math.round(distance/(4+5.6*v.character.stats.speed/75)*1000)))});
  }
  const before=await save('before-pickup');
  assert(Math.hypot(before.view.position.x-bag.pos.x,before.view.position.y-bag.pos.y)<.4,'Native walk did not reach Mage bag');
  assert.equal(before.view.character.items[4],-1);
  await act({keys:[],duration_ms:500},{name:'goblin-reward',when:()=>true});
  await act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await save('picked'),source=picked.diagnostic.entities.find(e=>e.id===bag.id);
  game.check('native drag commits the earned Mage item exactly once',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===before.view.character.inventory_revision+1&&(!source||source.items[0]===-1));
  for(let n=0;n<8;n++)await act({keys:[],duration_ms:2000});
  const end=await save('orphans');
  game.check('dead Mage cannot replenish escorts or transform',family(end).every(e=>e.kind!==0x614&&remaining.some(p=>p.id===e.id)));
  game.check('both surviving orphan roles continue moving',[0x612,0x613].every(kind=>family(end).some(e=>e.kind===kind&&remaining.some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.1))));
  game.check('Mage XP is awarded once',end.view.character.experience===dead.view.character.experience);
  await act({keys:['Insert'],duration_ms:250});
  const retained=await game.observe();await writeFile(join(process.env.QA_RESULTS_DIR,'nexus-retained.json'),JSON.stringify(retained));
  game.check('ordinary Nexus reconnect persists reward and living character',retained.world==='Nexus'&&retained.lifecycle.alive&&retained.session.character===begin.view.session.character&&JSON.stringify(retained.character.items)===JSON.stringify(picked.view.character.items)&&retained.character.inventory_revision===picked.view.character.inventory_revision&&retained.character.experience===end.view.character.experience);
  await act({keys:[],duration_ms:500},{name:'goblin-reward-retained',when:()=>true});
}
