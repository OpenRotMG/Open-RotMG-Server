import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const out=process.env.QA_RESULTS_DIR;
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
  const candidates=shamans.filter(e=>Array.from({length:7},(_,i)=>!blocked.has([Math.floor(e.pos.x)-6+i,Math.floor(e.pos.y)].join(','))).every(Boolean));
  const clearance=e=>Math.min(...map.entities.filter(p=>p.id!==e.id&&p.hp>0&&p.kind>=0x600).map(p=>Math.hypot(p.pos.x-e.pos.x,p.pos.y-e.pos.y)));
  shaman=candidates.sort((a,b)=>clearance(b)-clearance(a))[0];assert(shaman,'No naturally placed Shaman with a clear approach');
  await writeFile(join(out,'placement.json'),JSON.stringify({world,shaman,clearance:clearance(shaman),naturalShamans:map.entities.filter(e=>e.kind===0x6c6).length}));
  return {...spec,position:{x:shaman.pos.x-6,y:shaman.pos.y}};
}
export async function run(game){
  const save=async(name,state)=>writeFile(join(out,name+'.json'),JSON.stringify(state));
  const entity=s=>s.diagnostic.entities.find(e=>e.id===shaman.id);
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect();await save('arrival',initial);
  game.check('natural solo MidSand Shaman enters native view with selected 140 HP',entity(initial)?.hp===140&&entity(initial).camp===null&&entity(initial).quest===null&&initial.view.tiles.some(t=>t.ground===189));
  const samples=[];let blind=false,damage=false;
  for(let n=0;n<36;n++){
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
  game.check('native Shaman activates and moves toward nearby player',samples.some(s=>entity(s)?.activated!==null&&entity(s).pos.x<shaman.pos.x-1));
  game.check('unprotected native player receives encounter damage',damage);
  game.check('native spinner collision applies selected Blind',blind);
  const shots=samples.flatMap(s=>s.view.projectiles.filter(p=>p.owner===shaman.id));
  game.check('both selected projectile speeds and lifetimes execute',[55,60].every(speed=>shots.some(p=>p.speed===speed&&p.lifetime_ms===2600)));
  const births=new Map();
  for(const s of samples)for(const p of s.view.projectiles.filter(p=>p.owner===shaman.id)){
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
  game.check('native attack deadlines retain primary phase entry delays and cooldowns',[...births.values()].every(b=>{
    const phase=b.age%5300;
    return b.speed===55?phase>=500&&phase<3100&&phase%500===0:phase>=3800&&phase<5300&&(phase-3100)%700===0;
  })&&[3800,4500].every(age=>births.has('60:'+age)));
  const xp=(await game.observe()).character.experience;
  for(let n=0;n<50&&entity(await game.inspect());n++)await game.act({keys:[],duration_ms:250,target:{entity:shaman.id,camera_angle:-Math.PI/4,fire:true}});
  const dead=await game.inspect();await save('defeated',dead);
  game.check('ordinary native staff fire defeats Shaman and grants XP',!entity(dead)&&dead.view.character.experience>xp);
  await game.act({keys:[],duration_ms:1800},{name:'nomad-defeated',when:()=>true});
  const expired=await game.observe();
  game.check('Blind expires after defeat without reapplication',!(expired.character.conditions&(1<<7)));
  game.check('Shaman defeat XP remains once only',expired.character.experience===dead.view.character.experience);
  await game.act({keys:['Insert'],duration_ms:250});const nexus=await game.observe();
  game.check('ordinary Nexus transfer preserves living character and earned XP',nexus.world==='Nexus'&&nexus.lifecycle.alive&&nexus.session.character===initial.view.session.character&&nexus.character.experience===dead.view.character.experience);
}
