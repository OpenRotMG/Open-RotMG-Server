import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:802,world:1,equipment:[0xa9f,0xa2e,0xb05,0xac7],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
let queen;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'wasp-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  queen=map.entities.find(e=>e.kind===0x947&&[-2,-1,0,1,2].every(dx=>[-2,-1,0,1,2].every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));assert(queen,'No clear naturally populated Wasp Queen');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-queens.json'),JSON.stringify(map.entities.filter(e=>e.id===queen.id||e.parent===queen.id)));
  return {...spec,position:{x:queen.pos.x-2,y:queen.pos.y}};
}
export async function run(game){
  let serial=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  async function protect(){
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'wasp-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
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
  // XML MakeMinions Workers/Warriors (actors.xml:4679-4680) are owned by the Queen, not by the natural camp.
  const members=s=>s.diagnostic.entities.filter(e=>e.parent===queen.id&&[0x948,0x949].includes(e.kind)),owned=(s,kind)=>members(s).filter(e=>e.parent===queen.id&&e.kind===kind);
  const begin=await save('arrival'),hp=begin.view.character.hp;
  game.check('ordinary Realm Queen is visible with selected HP',begin.diagnostic.entities.some(e=>e.id===queen.id&&e.kind===0x947&&e.hp===850));
  // The dated Warrior subsection must not become Queen loot: https://web.archive.org/web/20120530043951id_/http://forums.wildshadow.com/wiki/realm-mad-god/wasp-queen
  game.check('ordinary Queen has no owned offspring before activation',!begin.diagnostic.entities.some(e=>e.parent===queen.id));
  await game.act({keys:[],duration_ms:1500},{name:'wasp-arrival',when:()=>true});
  const exposed=await save('exposed');
  game.check('activated Queen makes her XML five Workers and two Warriors',[[0x948,5],[0x949,2]].every(([k,n])=>owned(exposed,k).length===n));
  game.check('ordinary enemy projectiles cause native damage',exposed.view.character.hp<hp);
  await protect();
  // The XML Queen froths beside the player (actors.xml:4685-4689), so her 1200ms shots usually hit within ~300ms; sample densely.
  const samples=[];let pending=null,error;
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>samples.push(s)).catch(e=>{error=e;}).finally(()=>{pending=null;});},50);
  try{for(let n=0;n<6;n++)await act({keys:[],duration_ms:700});}finally{clearInterval(timer);if(pending)await pending;}
  if(error)throw error;
  await writeFile(join(process.env.QA_RESULTS_DIR,'movement-and-attacks.json'),JSON.stringify(samples));
  game.check('Queen activates and wanders',samples.some(s=>s.diagnostic.entities.some(e=>e.id===queen.id&&e.activated!==null&&Math.hypot(e.pos.x-queen.pos.x,e.pos.y-queen.pos.y)>.1)));
  for(const kind of [0x947,0x948,0x949]){
    const ids=new Set(kind===0x947?[queen.id]:owned(exposed,kind).map(e=>e.id));
    game.check(kind+': observed selected projectile speed and lifetime',samples.some(s=>s.view.projectiles.some(p=>ids.has(p.owner)&&p.speed===75&&p.lifetime_ms===1200)));
    if(kind!==0x947)game.check(kind+': owned followers move around their Queen',samples.some(s=>members(s).some(e=>e.kind===kind&&exposed.diagnostic.entities.some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.2))));
  }
  async function defeat(id){
    const before=await game.observe();
    // Five Workers and two Warriors refill every 3.4/4.4s around the Queen (actors.xml:4679-4680) and absorb her shots.
    for(let n=0;n<(id===queen.id?120:40);n++){
      const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===id),q=s.diagnostic.entities.find(e=>e.id===queen.id);if(!e)break;
      // XML Workers and Warriors Orbit the Queen at 2.5 (actors.xml:4706,4715): never fire through her while she must stay alive.
      const p=s.view.position,dx=e.pos.x-p.x,dy=e.pos.y-p.y,t=id===queen.id||!q?-1:((q.pos.x-p.x)*dx+(q.pos.y-p.y)*dy)/(dx*dx+dy*dy);
      const blocked=t>=0&&t<=1.3&&Math.hypot(q.pos.x-p.x-t*dx,q.pos.y-p.y-t*dy)<1.2,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      // The Queen can stay visible beyond staff range, and owned escorts can intercept fire.
      if(!s.view.objects.some(o=>o.id===id)||id===queen.id&&Math.hypot(dx,dy)>2.5)await act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:200});
      // The preceding shot may finish this follower before input resumes; aim at its observed bearing.
      else if(blocked)await act({keys:[],duration_ms:100});
      else {const scale=90/Math.max(1,Math.hypot(dx,dy));await act({keys:[],duration_ms:250,mouse:{x:300+x*scale,y:325+y*scale}});}
    }
    await act({keys:[],duration_ms:300});
    const s=await save('defeat-result-'+id);
    game.check(id+': native staff defeats target and awards XP',!s.diagnostic.entities.some(e=>e.id===id)&&s.view.character.experience>before.character.experience);
  }
  for(const kind of [0x948,0x949]){
    const before=await save('before-defeat-'+kind),victim=owned(before,kind)[0];assert(victim,'Queen-owned '+kind+' required');
    const ids=new Set(members(before).map(e=>e.id));await defeat(victim.id);
    for(let n=0;n<12&&!owned(await game.inspect(),kind).some(e=>!ids.has(e.id));n++)await act({keys:[],duration_ms:1000});
    const after=await save('replenished-'+kind);
    game.check(kind+': living Queen replenishes the defeated role',owned(after,kind).some(e=>!ids.has(e.id)));
    game.check(kind+': Queen-owned Workers and Warriors stay within five and two',[ [0x948,5],[0x949,2] ].every(([k,max])=>owned(after,k).length<=max));
  }
  await act({keys:[],duration_ms:500},{name:'wasp-replenished',when:()=>true});
  await defeat(queen.id);
  const dead=await save('queen-defeated'),remaining=members(dead);
  game.check('Queen defeat produces no successor',dead.diagnostic.entities.filter(e=>e.id===queen.id||e.parent===queen.id).every(e=>e.kind===0x948||e.kind===0x949));
  await act({keys:[],duration_ms:10000});
  await act({keys:[],duration_ms:1000},{name:'wasp-defeated',when:()=>true});
  const final=await save('after-parent-death');
  game.check('dead Queen cannot replenish her camp',members(final).every(e=>remaining.some(p=>p.id===e.id))&&!final.diagnostic.entities.some(e=>e.id===queen.id));
  game.check('Queen defeat XP is not awarded twice',final.view.character.experience===dead.view.character.experience);
  game.check('same living prepared Wizard completes encounter',final.view.lifecycle.alive&&final.view.session.character===begin.view.session.character);
}
