import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:808,world:1,equipment:[0xa9f,0xa2e,0xb05,0xac7],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
let king;
export async function prepare(api){
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  for(const id of [1,-3]){
    const world=await api({op:'world_info',world:id}),[x,y]=world.spawn_points[0];
    const scout=await api({op:'spawn',spec:{...spec,world:id,guid:'sandsman-scout-'+id,position:{x,y},controller:'direct',profile:null}});
    const state=await api({op:'inspect',actor:scout.result.actor});
    const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
    const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
    await writeFile(join(process.env.QA_RESULTS_DIR,'natural-kings-'+id+'.json'),JSON.stringify(map.entities.filter(e=>e.kind===0x60d)));
    const clear=(e,dx,dy)=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(','));
    // Prefer space for the 6.5 ± 0.5 tile orbit; the little west approach alone does not test it.
    const kings=map.entities.filter(e=>e.kind===0x60d&&Array.from({length:7},(_,n)=>n-6).every(dx=>clear(e,dx,0)));
    const offsets=Array.from({length:17},(_,n)=>n-8).flatMap(dx=>Array.from({length:17},(_,n)=>n-8).map(dy=>[dx,dy])).filter(([dx,dy])=>Math.hypot(dx,dy)<=8);
    const spaces=kings.map(e=>({e,open:offsets.filter(([dx,dy])=>clear(e,dx,dy)).length}));
    king=spaces.filter(s=>s.open>=.9*offsets.length).sort((a,b)=>b.open-a.open)[0]?.e;
    if(king)return {...spec,world:id,position:{x:king.pos.x-6,y:king.pos.y}};
  }
  throw Error('Neither ordinary Realm has a King with clear orbit space');
}
export async function run(game){
  let serial=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  async function protect(){
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'sandsman-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);shielded=(await game.observe()).game_ms+60000;
  }
  let shielded=null;
  // Protection cannot be renewed while active, and loaded acts can overrun: idle out the window, then renew.
  async function act(input,capture){
    let now=(await game.observe()).game_ms;
    while(shielded!==null&&now<shielded&&shielded-now<input.duration_ms+5000){await game.act({keys:[],duration_ms:Math.min(10000,Math.max(50,shielded-now))});now=(await game.observe()).game_ms;}
    if(shielded!==null&&now>=shielded)await protect();
    return game.act(input,capture);
  }
  // XML King MakeMinions escorts (actors.xml:427-428) are owned by parent, not by the natural camp.
  const family=s=>s.diagnostic.entities.filter(e=>(e.id===king.id||e.parent===king.id)&&[0x60d,0x60a,0x60c].includes(e.kind)),owned=(s,kind)=>family(s).filter(e=>e.parent===king.id&&e.kind===kind);
  const begin=await save('arrival'),hp=begin.view.character.hp;
  game.check('ordinary Realm King has selected 270 HP',family(begin).some(e=>e.id===king.id&&e.hp===270));
  // Recovered realm/low.xml:95–96: offspring belong to the XML spawner, not an ordinary population slot.
  game.check('ordinary root is placed before it owns any offspring',begin.diagnostic.entities.some(e=>e.id===king.id&&e.camp===null&&e.parent===null)&&!begin.diagnostic.entities.some(e=>e.parent===king.id));
  // The King shoots at first sight and then every 10s (actors.xml:426); his 1200ms shot is brief, so sample densely from arrival.
  const samples=[];let pending=null,error,escorts,approachStart,approachEnd;
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>samples.push(s)).catch(e=>{error=e;}).finally(()=>{pending=null;});},50);
  try{
    await game.act({keys:[],duration_ms:1800},{name:'sandsman-before',when:()=>true});
    for(let n=0;n<10&&(await game.observe()).character.hp>=hp;n++)await game.act({keys:[],duration_ms:1000});
    game.check('ordinary encounter damages the native character',(await game.observe()).character.hp<hp);
    escorts=await save('escorts');
    game.check('activated King makes its XML two Archers and three Sorcerers (actors.xml:427-428)',[[0x60a,2],[0x60c,3]].every(([k,n])=>owned(escorts,k).length===n));
    await protect();
    // Let the Archer first reach its radius, then sample a substantial arc.
    for(let n=0;n<8;n++)await act({keys:[],duration_ms:5000});
    // The second Sorcerer attack is range five, unlike its long-lived Sand Cloud (realm/low.xml:58–59).
    approachStart=(await game.observe()).position;
    const sorcerers=new Set(owned(escorts,0x60c).map(e=>e.id));
    for(let n=0;n<50;n++){
      const s=await game.inspect(),p=s.view.position,e=owned(s,0x60c).sort((a,b)=>Math.hypot(a.pos.x-p.x,a.pos.y-p.y)-Math.hypot(b.pos.x-p.x,b.pos.y-p.y))[0];
      assert(e,'Living owned Sorcerer required');
      const dx=e.pos.x-p.x,dy=e.pos.y-p.y,x=dx-dy,y=dx+dy;
      const distance=Math.hypot(dx,dy);
      if(distance<4.5&&samples.some(s=>s.view.projectiles.some(p=>sorcerers.has(p.owner)&&p.speed===80)))break;
      const keys=distance<3?[]:[Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean);
      await act({keys,duration_ms:250});
    }
    approachEnd=await game.inspect();
  }finally{clearInterval(timer);if(pending)await pending;}
  if(error)throw error;
  await writeFile(join(process.env.QA_RESULTS_DIR,'movement-and-attacks.json'),JSON.stringify(samples));
  await act({keys:[],duration_ms:500},{name:'sandsman-orbit',when:()=>true});
  game.check('King activates and moves',samples.some(s=>family(s).some(e=>e.id===king.id&&e.activated!==null&&Math.hypot(e.pos.x-king.pos.x,e.pos.y-king.pos.y)>.1)));
  for(const [kind,speed,lifetime] of [[0x60d,70,1200],[0x60a,85,1400],[0x60c,8,10000],[0x60c,80,300]]){
    const ids=new Set(family(escorts).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': native session receives projectile '+speed+'/'+lifetime,samples.some(s=>s.view.projectiles.some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime)));
  }
  const orbit=family(escorts).filter(e=>e.kind===0x60a).map(a=>{
    let last=null,angle=0,near=0;
    // Do not infer an arc across gaps where the wandering King or Archer left the observed neighborhood.
    for(const s of samples.filter(s=>s.view.time_ms>=escorts.view.time_ms)){const k=family(s).find(e=>e.id===king.id),e=family(s).find(e=>e.id===a.id);if(!k||!e){last=null;continue;}const dx=e.pos.x-k.pos.x,dy=e.pos.y-k.pos.y,t=Math.atan2(dy,dx);if(last!==null)angle+=Math.atan2(Math.sin(t-last),Math.cos(t-last));last=t;if(Math.abs(Math.hypot(dx,dy)-6.5)<1)near++;}
    return {id:a.id,angle,near};
  });
  await writeFile(join(process.env.QA_RESULTS_DIR,'orbit.json'),JSON.stringify(orbit));
  const position=approachEnd.view.position;
  game.check('native approach reaches a living escort inside its recovered five-tile range',
    owned(approachEnd,0x60c).some(e=>Math.hypot(e.pos.x-position.x,e.pos.y-position.y)<5)&&
    (Math.hypot(position.x-approachStart.x,position.y-approachStart.y)>.5||
      samples.some(s=>owned(s,0x60c).some(e=>Math.hypot(e.pos.x-s.view.position.x,e.pos.y-s.view.position.y)<5))));
  const dist=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  const keysToward=(p,q)=>{const dx=q.x-p.x,dy=q.y-p.y,x=dx-dy,y=dx+dy;return [Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean);};
  const clearsKing=(p,q,k)=>{const dx=q.x-p.x,dy=q.y-p.y,t=((k.x-p.x)*dx+(k.y-p.y)*dy)/(dx*dx+dy*dy);return t<0||t>1||Math.hypot(k.x-p.x-t*dx,k.y-p.y-t*dy)>1.5;};
  async function defeat(actor, kind){
    const xp=(await game.observe()).character.experience;
    let id=actor,fired=false;
    for(let n=0;n<80;n++){
      const s=await game.inspect(),p=s.view.position,k=family(s).find(e=>e.id===king.id);
      const previous=family(s).find(e=>e.id===id);
      if(id&&!previous)break;
      assert(k,'King must be in view while selecting a native firing line');
      const choices=(kind?owned(s,kind):[previous]).sort((a,b)=>dist(a.pos,p)-dist(b.pos,p));
      const visible=e=>s.view.objects.some(o=>o.id===e.id&&o.hp>0);
      const line=e=>visible(e)&&dist(e.pos,p)<4&&(!kind||clearsKing(p,e.pos,k.pos));
      const e=fired?previous:choices.find(line)??choices[0];
      assert(e,'Selected escort must stay in view while closing for native fire');
      if(line(e)){
        id=e.id;fired=true;
        const dx=e.pos.x-p.x,dy=e.pos.y-p.y,scale=64/Math.max(1,dist(e.pos,p));
        await act({keys:[],duration_ms:250,mouse:{x:300+(dx-dy)*scale,y:325+(dx+dy)*scale}});
      }else{
        // The Archer orbits at 6.5 tiles; a stationary wait can leave every escort behind cover (AC 54822f98).
        const separation=dist(e.pos,k.pos);
        const pos=kind&&separation>0&&!clearsKing(p,e.pos,k.pos)?{x:e.pos.x+2*(e.pos.x-k.pos.x)/separation,y:e.pos.y+2*(e.pos.y-k.pos.y)/separation}:e.pos;
        await act({keys:keysToward(p,pos),duration_ms:250});
      }
    }
    await act({keys:[],duration_ms:700});const s=await game.inspect();
    game.check(id+': native staff defeats target and earns XP',fired&&!s.diagnostic.entities.some(e=>e.id===id)&&s.view.character.experience>xp);
  }
  for(const kind of [0x60a,0x60c]){
    const before=await save('before-defeat-'+kind);
    const ids=new Set(family(before).map(e=>e.id));await defeat(null,kind);
    for(let n=0;n<12&&!owned(await game.inspect(),kind).some(e=>!ids.has(e.id));n++)await act({keys:[],duration_ms:1000});
    const after=await save('replenished-'+kind);
    game.check(kind+': living King replenishes defeated escort',owned(after,kind).some(e=>!ids.has(e.id)));
    game.check(kind+': King-owned escorts respect XML caps',[[0x60a,2],[0x60c,3]].every(([k,n])=>owned(after,k).length<=n));
  }
  await defeat(king.id);const dead=await save('king-defeated'),remaining=family(dead);
  await act({keys:[],duration_ms:10000});
  await act({keys:[],duration_ms:1000},{name:'sandsman-defeated',when:()=>true});const final=await save('after-parent-death');
  game.check('dead King cannot replenish or transform',family(final).every(e=>e.kind!==0x60d&&remaining.some(p=>p.id===e.id)));
  game.check('King XP cannot be awarded twice',final.view.character.experience===dead.view.character.experience);
  game.check('same prepared native Wizard survives',final.view.lifecycle.alive&&final.view.session.character===begin.view.session.character);
  // XML Archers Orbit the King at radius 6.5 with variability 0.5 (actors.xml:406); scenery can pin one natural Archer, and orbit direction is unspecified.
  game.check('an Archer circles its King at the XML orbit radius',orbit.some(a=>Math.abs(a.angle)>2&&a.near>=12));
}
