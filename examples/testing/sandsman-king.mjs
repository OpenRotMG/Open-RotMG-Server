import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:808,world:1,equipment:[0xa9f,0xa2e,0xb05,0xac7],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
let king;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'sandsman-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  king=map.entities.find(e=>e.kind===0x60d&&Array.from({length:13},(_,i)=>i-6).every(dx=>Array.from({length:13},(_,i)=>i-6).every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));assert(king,'No clear naturally populated Sandsman King');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-kings.json'),JSON.stringify(map.entities.filter(e=>e.kind===0x60d)));
  return {...spec,position:{x:king.pos.x-6,y:king.pos.y}};
}
export async function run(game){
  let serial=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  async function protect(){
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'sandsman-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);
  }
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===king.camp&&[0x60d,0x60a,0x60c].includes(e.kind));
  const begin=await save('arrival'),hp=begin.view.character.hp;
  game.check('ordinary Realm King has selected 270 HP',family(begin).some(e=>e.id===king.id&&e.hp===270));
  game.check('ordinary camp has two Archers and three Sorcerers',[[0x60a,2],[0x60c,3]].every(([k,n])=>family(begin).filter(e=>e.kind===k).length===n));
  await game.act({keys:[],duration_ms:1800},{name:'sandsman-before',when:()=>true});
  game.check('ordinary encounter damages the native character',(await game.observe()).character.hp<hp);
  await protect();
  const samples=[];
  for(let n=0;n<18;n++){await game.act({keys:[],duration_ms:600});samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'movement-and-attacks.json'),JSON.stringify(samples));
  await game.act({keys:[],duration_ms:500},{name:'sandsman-orbit',when:()=>true});
  game.check('King activates and moves',samples.some(s=>family(s).some(e=>e.id===king.id&&e.activated!==null&&Math.hypot(e.pos.x-king.pos.x,e.pos.y-king.pos.y)>.1)));
  for(const [kind,speed,lifetime] of [[0x60d,70,1200],[0x60a,85,1400],[0x60c,8,10000],[0x60c,80,300]]){
    const ids=new Set(family(begin).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': native session receives projectile '+speed+'/'+lifetime,samples.some(s=>s.view.projectiles.some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime)));
  }
  const orbit=family(begin).filter(e=>e.kind===0x60a).map(a=>{
    let last=null,angle=0,near=0;
    for(const s of samples){const k=family(s).find(e=>e.id===king.id),e=family(s).find(e=>e.id===a.id);assert(k&&e);const dx=e.pos.x-k.pos.x,dy=e.pos.y-k.pos.y,t=Math.atan2(dy,dx);if(last!==null)angle+=Math.atan2(Math.sin(t-last),Math.cos(t-last));last=t;if(Math.hypot(dx,dy)>1.5&&Math.hypot(dx,dy)<4.5)near++;}
    return {id:a.id,angle,near};
  });
  await writeFile(join(process.env.QA_RESULTS_DIR,'orbit.json'),JSON.stringify(orbit));
  const position=(await game.observe()).position;
  await game.act({keys:['d','s'],duration_ms:350});
  game.check('native movement enters the escort group',Math.hypot((await game.observe()).position.x-position.x,(await game.observe()).position.y-position.y)>.5);
  async function defeat(id){
    const xp=(await game.observe()).character.experience;
    for(let n=0;n<25&&(await game.inspect()).view.objects.some(e=>e.id===id&&e.hp>0);n++)await game.act({keys:[],duration_ms:100,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    await game.act({keys:[],duration_ms:700});const s=await game.inspect();
    game.check(id+': native staff defeats target and earns XP',!s.diagnostic.entities.some(e=>e.id===id)&&s.view.character.experience>xp);
  }
  for(const kind of [0x60a,0x60c]){
    const before=await save('before-defeat-'+kind),p=before.view.position;
    const k=family(before).find(e=>e.id===king.id);assert(k,'King must be alive before escort targeting');
    const safe=e=>{const dx=e.pos.x-p.x,dy=e.pos.y-p.y,t=((k.pos.x-p.x)*dx+(k.pos.y-p.y)*dy)/(dx*dx+dy*dy);return t<0||t>1||Math.hypot(k.pos.x-p.x-t*dx,k.pos.y-p.y-t*dy)>1.5;};
    const victim=family(before).filter(e=>e.kind===kind&&safe(e)&&before.view.objects.some(o=>o.id===e.id&&o.hp>0)).sort((a,b)=>Math.hypot(a.pos.x-p.x,a.pos.y-p.y)-Math.hypot(b.pos.x-p.x,b.pos.y-p.y))[0];assert(victim,'No visible escort with a clear firing line away from the King');
    const ids=new Set(family(before).map(e=>e.id));await defeat(victim.id);
    for(let n=0;n<12&&!family(await game.inspect()).some(e=>e.kind===kind&&!ids.has(e.id));n++)await game.act({keys:[],duration_ms:1000});
    const after=await save('replenished-'+kind);
    game.check(kind+': living King replenishes defeated escort',family(after).some(e=>e.kind===kind&&!ids.has(e.id)));
    game.check(kind+': observed camp respects current role caps',[[0x60a,2],[0x60c,3]].every(([k,n])=>family(after).filter(e=>e.kind===k).length<=n));
  }
  await defeat(king.id);const dead=await save('king-defeated'),remaining=family(dead);
  await game.act({keys:[],duration_ms:11000},{name:'sandsman-defeated',when:()=>true});const final=await save('after-parent-death');
  game.check('dead King cannot replenish or transform',family(final).every(e=>e.kind!==0x60d&&remaining.some(p=>p.id===e.id)));
  game.check('King XP cannot be awarded twice',final.view.character.experience===dead.view.character.experience);
  game.check('same prepared native Wizard survives',final.view.lifecycle.alive&&final.view.session.character===begin.view.session.character);
  game.check('both Archers circle their King rather than wandering',orbit.every(a=>a.angle>2&&a.near>=12));
}
