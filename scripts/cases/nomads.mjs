import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const out=process.env.QA_RESULTS_DIR;
export const realmSeed=12351;
const spec={class:0x30e,level:20,seed:833,world:1,equipment:[0xa97,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,25,0,50,20,0,0]};
let shaman;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'nomad-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(out,'native/result.json'),'utf8'));
  const token=(await readFile(join(out,'native/state/control.token'),'utf8')).trim();
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const shamans=map.entities.filter(e=>e.kind===0x6c6);
  await writeFile(join(out,'natural-shamans.json'),JSON.stringify(shamans));
  const close=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  const candidates=shamans.flatMap(e=>[[-1,0],[1,0],[0,-1],[0,1]].flatMap(([dx,dy])=>{
    if(Array.from({length:7},(_,i)=>[Math.floor(e.pos.x)+i*dx,Math.floor(e.pos.y)+i*dy].join(',')).some(cell=>blocked.has(cell)))return [];
    const position={x:e.pos.x+6*dx,y:e.pos.y+6*dy};
    const clearance=Math.min(...map.entities.filter(p=>p.id!==e.id&&p.hp>0&&p.kind>=0x600).map(p=>Math.min(close(p.pos,e.pos),close(p.pos,position))));
    return [{e,position,clearance}];
  }));
  // The normal killable witness must be separated from unrelated natural attacks.
  const selected=candidates.sort((a,b)=>b.clearance-a.clearance)[0];assert(selected,'No naturally placed Shaman with a clear approach');
  const {e,position,clearance}=selected;shaman=e;
  await writeFile(join(out,'placement.json'),JSON.stringify({world,shaman,position,clearance,naturalShamans:shamans.length}));
  return {...spec,position};
}
export async function run(game){
  const save=async(name,state)=>writeFile(join(out,name+'.json'),JSON.stringify(state));
  const entity=s=>s.diagnostic.entities.find(e=>e.id===shaman.id);
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect();await save('arrival',initial);
  game.check('natural solo MidSand Shaman enters native view with selected 140 HP',entity(initial)?.hp===140&&entity(initial).camp===null&&entity(initial).quest===null&&initial.view.tiles.some(t=>t.ground===189));
  const samples=[];let blind=false,damage=false;
  // The XML Shaman only Wanders, so keep sampling until its spinner has actually hit and blinded the player.
  for(let n=0;n<36||n<80&&!(blind&&damage);n++){
    await game.act({keys:[],duration_ms:200},n===5?{name:'nomad-encounter',when:()=>true}:undefined);
    const s=await game.inspect();samples.push(s);
    damage ||= s.view.character.hp<initial.view.character.hp;
    if((s.view.character.conditions&(1<<7))&&!blind){
      blind=true;await game.act({keys:[],duration_ms:100},{name:'nomad-blind',when:v=>Boolean(v.character.conditions&(1<<7))});
      const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'nomad-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}})});
      const receipt=await response.json();await save('protection',receipt);assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
    }
    assert(s.view.lifecycle.alive,'Prepared Wizard died before encounter observations');
  }
  await save('combat',samples);
  // The XML Shaman only Wanders (actors.xml:2307); base chased the player.
  game.check('native Shaman activates and wanders',samples.some(s=>entity(s)&&entity(s).activated!==null&&Math.hypot(entity(s).pos.x-shaman.pos.x,entity(s).pos.y-shaman.pos.y)>.2));
  game.check('unprotected native player receives encounter damage',damage);
  game.check('native spinner collision applies selected Blind',blind);
  const shots=samples.flatMap(s=>s.view.projectiles.filter(p=>p.owner===shaman.id));
  game.check('both selected projectile speeds and lifetimes execute',[55,60].every(speed=>shots.some(p=>p.speed===speed&&p.lifetime_ms===2600)));
  const births=new Map();
  for(const s of samples.filter(entity))for(const p of s.view.projectiles.filter(p=>p.owner===shaman.id)){
    const age=Math.round((s.view.time_ms-Math.hypot(p.position.x-p.start.x,p.position.y-p.start.y)/(p.speed/10000)-entity(s).activated)/50)*50;
    births.set(p.speed+':'+age,{speed:p.speed,age});
  }
  await save('volley-times',[...births.values()]);
  const fans=samples.flatMap(s=>{
    const groups=new Map();
    for(const p of s.view.projectiles.filter(p=>p.owner===shaman.id&&p.speed===55)){
      const age=Math.hypot(p.position.x-p.start.x,p.position.y-p.start.y)/.0055,key=Math.round((s.view.time_ms-age)/50);
      if(!groups.has(key))groups.set(key,[]);groups.get(key).push(p.angle);
    }
    return [...groups.values()].filter(g=>g.length===3).map(g=>g.sort((a,b)=>a-b));
  });
  game.check('native Green Magic fan uses primary 11-degree adjacent spacing',fans.length>0&&fans.every(g=>g.slice(1).every((a,i)=>Math.abs(a-g[i]-11*Math.PI/180)<.002)));
  // XML fire1 shoots every 0.5s from 0.5s for 3.1s, fire2 every 0.7s from 0.7s for 2.2s (actors.xml:2308-2315).
  // The 5.3s cycle is anchored on the executor clock, which can start a tick before activation is recorded; align on the 0.5s fire1 grid.
  const shift=(500-Math.min(...[...births.values()].filter(b=>b.speed===55).map(b=>b.age))%500)%500,phase=b=>(b.age+shift)%5300;
  // Verify two complete cycles before a wandering Shaman can lose its range-10 target and defer later shots.
  const cycles=[0,1].map(n=>[...births.values()].filter(b=>Math.floor((b.age+shift)/5300)===n));
  game.check('native attack timing follows XML state offsets and cooldowns',cycles.every(c=>[55,60].every(speed=>c.some(b=>b.speed===speed))&&c.every(b=>b.speed===55?[500,1000,1500,2000,2500,3000].includes(phase(b)):[3800,4500,5200].includes(phase(b)))));
  const xp=(await game.observe()).character.experience;
  // The XML Shaman only Wanders (actors.xml:2307), so close in before firing.
  for(let n=0;n<100;n++){
    const s=await game.inspect(),e=entity(s);if(!e)break;
    const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
    await game.act(Math.hypot(dx,dy)>4?{keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:200}:{keys:[],duration_ms:250,target:{entity:shaman.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const dead=await game.inspect();await save('defeated',dead);
  game.check('ordinary native staff fire defeats Shaman and grants XP',!entity(dead)&&dead.view.character.experience>xp);
  await game.act({keys:[],duration_ms:1800},{name:'nomad-defeated',when:()=>true});
  const expired=await game.observe();
  game.check('Blind expires after defeat without reapplication',!(expired.character.conditions&(1<<7)));
  game.check('Shaman defeat XP remains once only',expired.character.experience===dead.view.character.experience);
  await game.act({keys:['Insert'],duration_ms:250});const nexus=await game.observe();
  game.check('ordinary Nexus transfer preserves living character and earned XP',nexus.world==='Nexus'&&nexus.lifecycle.alive&&nexus.session.character===initial.view.session.character&&nexus.character.experience===dead.view.character.experience);
}
