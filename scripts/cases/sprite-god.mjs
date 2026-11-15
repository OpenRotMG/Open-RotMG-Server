import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const output=name=>join(process.env.QA_RESULTS_DIR,name);
const spec={class:0x30e,level:20,seed:852,world:1,equipment:[0xb08,0xb24,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,0,0]};
let root;
export async function prepare(api){
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'sprite-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+(await readFile(output('native/state/control.token'),'utf8')).trim()}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const candidates=map.entities.filter(e=>e.kind===0x652).filter(e=>{
    for(let dx=-8;dx<=8;dx++)for(let dy=-5;dy<=5;dy++)if(blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))return false;
    return true;
  }).map(e=>({entity:e,clearance:Math.min(...map.entities.filter(o=>o.hp>0&&o.id!==e.id&&o.kind!==0x653).map(o=>Math.hypot(o.pos.x-e.pos.x,o.pos.y-e.pos.y)))})).sort((a,b)=>b.clearance-a.clearance);
  assert(candidates.length,'No naturally placed Sprite God has a clear approach');root=candidates[0].entity;
  await writeFile(output('natural-placement.json'),JSON.stringify({world:info,root,candidates}));
  return {...spec,position:{x:root.pos.x-5,y:root.pos.y}};
}
export async function run(game){
  const config=JSON.parse(await readFile(output('native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  let protectionUntil=0;
  const enemy=(s,id=root.id)=>s.diagnostic.entities.find(e=>e.id===id);
  const children=s=>s.diagnostic.entities.filter(e=>e.kind===0x653&&e.camp===root.camp);
  async function protect(condition,duration_ms){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition,duration_ms}}})});
    const value=await response.json();assert(response.ok&&value.status==='succeeded',JSON.stringify(value));
    protectionUntil=(await game.observe()).game_ms+duration_ms;
  }
  async function act(input,capture){
    let pending,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>samples.push(s)).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();samples.push(s);return s;
  }
  async function kill(id){
    for(let n=0;n<100;n++){
      const s=await game.inspect(),e=enemy(s,id);if(!e)return;
      if((await game.observe()).game_ms>=protectionUntil)await protect('invulnerable',60000);
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,distance=Math.hypot(dx,dy),approach=distance>(id===root.id?1:4.5);
      await act({keys:approach?[Math.abs(x)>.2?(x>0?'d':'a'):null,Math.abs(y)>.2?(y>0?'s':'w'):null].filter(Boolean):[],duration_ms:approach?Math.min(250,Math.max(50,Math.round(distance*60))):250,target:{entity:id,camera_angle:-Math.PI/4,fire:!approach}});
    }
    assert.fail('Native fire did not defeat '+id);
  }
  try{
    const arrival=await game.inspect();root=enemy(arrival);assert(root);
    game.check('natural mountain God and three owned children start at selected HP',root.hp===2000&&children(arrival).length===3&&children(arrival).every(e=>e.hp===50));
    await protect('invulnerable',60000);
    await act({keys:[],duration_ms:5000});
    const quiet=await act({keys:[],duration_ms:1500},{name:'sprite-god-quiet',when:v=>(v.character.conditions&2)!==0});
    game.check('zero-damage boomerang applies Quiet and drains MP through HP protection',(quiet.view.character.conditions&2)!==0&&quiet.view.character.mp===0&&quiet.view.character.hp===arrival.view.character.hp);
    const shots=samples.flatMap(s=>s.view.projectiles).filter(p=>p.owner===root.id);
    game.check('native receives both selected projectile trajectories',shots.some(p=>p.speed===70&&p.lifetime_ms===2000)&&shots.some(p=>p.speed===60&&p.lifetime_ms===2400));
    // Build118 documents both attacks and the non-attacking children, not exact123.5.1 AI: https://web.archive.org/web/20120528162648id_/http://forums.wildshadow.com/wiki/realm-mad-god/sprite-god
    game.check('Purple Magic forms four-shot fans with ten-degree gaps',samples.some(s=>{
      const groups=new Map();for(const p of s.view.projectiles.filter(p=>p.owner===root.id&&p.speed===70)){const key=p.start.x+':'+p.start.y;if(!groups.has(key))groups.set(key,[]);groups.get(key).push(p.angle);}
      return [...groups.values()].some(a=>a.length===4&&a.sort((a,b)=>a-b).every((v,i)=>i===0||Math.abs(v-a[i-1]-Math.PI/18)<.001));
    }));
    const initialChildren=children(arrival);
    game.check('God and its children activate and move without child attacks',samples.some(s=>{const e=enemy(s);return e?.activated!==null&&e&&Math.hypot(e.pos.x-root.pos.x,e.pos.y-root.pos.y)>.2;})&&initialChildren.every(c=>samples.some(s=>{const e=enemy(s,c.id);return e&&Math.hypot(e.pos.x-c.pos.x,e.pos.y-c.pos.y)>.2;}))&&!samples.some(s=>s.view.projectiles.some(p=>initialChildren.some(c=>p.owner===c.id))));
    const child=children(quiet)[0],xp=quiet.view.character.experience;
    await kill(child.id);const childDead=await game.inspect();
    game.check('ordinary native fire defeats a child and earns its five XP',!enemy(childDead,child.id)&&childDead.view.character.experience>=xp+5);
    await act({keys:[],duration_ms:10000});const regrown=await game.inspect();
    game.check('bounded owned family regrows a new child while parent lives',children(regrown).some(e=>!initialChildren.some(c=>c.id===e.id))&&children(regrown).length<=5);
    const beforeKill=await game.inspect();await kill(root.id);
    const dead=await act({keys:[],duration_ms:500},{name:'sprite-god-defeated',when:()=>true});
    game.check('ordinary native fire defeats parent and earns level-capped195 XP',!enemy(dead)&&dead.view.character.experience>=beforeKill.view.character.experience+195);
    const orphan=children(dead)[0];assert(orphan,'Parent defeat should leave surviving children');
    await act({keys:[],duration_ms:10000});await act({keys:[],duration_ms:5000});const orphaned=await game.inspect();
    game.check('orphan survives and wanders without new family births after parent death',enemy(orphaned,orphan.id)&&Math.hypot(enemy(orphaned,orphan.id).pos.x-orphan.pos.x,enemy(orphaned,orphan.id).pos.y-orphan.pos.y)>.2&&children(orphaned).every(e=>children(dead).some(c=>c.id===e.id)));
    game.check('Quiet expires and MP regeneration resumes after parent defeat',!(orphaned.view.character.conditions&2)&&orphaned.view.character.mp>0);
    game.check('parent removal cannot award XP twice',!enemy(orphaned)&&orphaned.view.character.experience===dead.view.character.experience);
    await kill(orphan.id);const end=await game.inspect();
    game.check('orphan remains killable and same prepared Wizard survives',!enemy(end,orphan.id)&&end.view.lifecycle.alive&&end.view.session.character===arrival.view.session.character);
    await writeFile(output('outcomes.json'),JSON.stringify({arrival,quiet,childDead,regrown,dead,orphaned,end}));
  }finally{await writeFile(output('combat-samples.json'),JSON.stringify(samples));}
}
