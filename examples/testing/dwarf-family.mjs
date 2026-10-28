import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:5,seed:805,world:1,equipment:[0xa97,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,25,0,30,20,0,0]};
let king;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'dwarf-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),root=join(process.env.QA_RESULTS_DIR,'native');
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  king=map.entities.find(e=>e.kind===0x61f&&Array.from({length:9},(_,i)=>[-1,0,1].every(j=>!blocked.has([Math.floor(e.pos.x)-i,Math.floor(e.pos.y)+j].join(',')))).every(Boolean));
  assert(king,'No natural Dwarf King with a clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify(map.entities.filter(e=>e.camp===king.camp)));
  return {...spec,position:{x:king.pos.x-8,y:king.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===king.camp),entity=(s,id)=>family(s).find(e=>e.id===id);
  let protectedUntil=0;
  const act=async(input,capture)=>{
    await protect();
    while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await game.act({keys:[],duration_ms:100});await protect();}
    return game.act(input,capture);
  };
  const begin=await save('arrival');
  game.check('natural MidForest family has selected initial HP and composition',[[0x61f,1,320],[0x61c,2,80],[0x61d,1,80],[0x61e,1,100]].every(([k,n,hp])=>family(begin).filter(e=>e.kind===k&&e.hp===hp).length===n));
  for(let n=0;n<16&&(await game.observe()).character.hp===begin.view.character.hp;n++)await game.act({keys:[],duration_ms:500});
  game.check('unprotected native Wizard receives encounter damage',(await game.observe()).character.hp<begin.view.character.hp);
  const protect=async()=>{
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const path=join(process.env.QA_RESULTS_DIR,'protection.json');
    await writeFile(path,JSON.stringify({id:'dwarf-protection-'+Date.now(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);protectedUntil=now+60000;
  };
  await protect();
  const samples=[];
  for(let n=0;n<32;n++){await act({keys:[],duration_ms:500},n===16?{name:'dwarf-encounter',when:()=>true}:undefined);samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'movement-and-attacks.json'),JSON.stringify(samples));
  for(const [kind,speed,lifetime] of [[0x61f,60,900],[0x61c,60,600],[0x61d,35,3500],[0x61e,60,600]]){
    const ids=new Set(family(begin).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': natural actor activates and moves',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null&&Math.hypot(e.pos.x-family(begin).find(p=>p.id===e.id).pos.x,e.pos.y-family(begin).find(p=>p.id===e.id).pos.y)>.2)));
    game.check(kind+': selected projectile executes during native combat',samples.some(s=>s.view.projectiles.some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime)));
  }
  const mages=new Set(samples.flatMap(family).filter(e=>e.kind===0x61d).map(e=>e.id));
  // The selected amplitude/frequency use lateral sine displacement, not the separate Wavy flag: https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/com/company/assembleegameclient/objects/Projectile.as
  const waves=samples.flatMap(s=>s.view.projectiles.filter(p=>mages.has(p.owner)).map(p=>{
    const dx=p.position.x-p.start.x,dy=p.position.y-p.start.y,along=dx*Math.cos(p.angle)+dy*Math.sin(p.angle),across=-dx*Math.sin(p.angle)+dy*Math.cos(p.angle);
    const age=along/0.0035,expected=Math.sin((p.id%2)*Math.PI+age/3500*3*2*Math.PI);
    assert(p.amplitude===1&&p.frequency===3&&Math.abs(across-expected)<.003,'Mage trajectory differs from selected sine');
    return {owner:p.owner,id:p.id,age,across,expected};
  }));
  await writeFile(join(process.env.QA_RESULTS_DIR,'mage-trajectories.json'),JSON.stringify(waves));
  game.check('Mage shots execute both sides of the selected sinusoidal trajectory',waves.some(p=>p.across>.5)&&waves.some(p=>p.across<-.5));
  const caps=s=>[[0x61c,3],[0x61d,2],[0x61e,2]].every(([k,n])=>family(s).filter(e=>e.kind===k).length<=n);
  game.check('current escort caps hold throughout observed combat',samples.every(caps));
  async function defeat(id){
    const xp=(await game.observe()).character.experience;
    for(let n=0;n<80;n++){
      const s=await game.inspect(),e=entity(s,id);if(!e)break;
      const k=entity(s,king.id);assert(k,'King must survive escort targeting');
      const p=s.view.position,dx=e.pos.x-p.x,dy=e.pos.y-p.y,t=((k.pos.x-p.x)*dx+(k.pos.y-p.y)*dy)/(dx*dx+dy*dy);
      if(id===king.id||t<0||Math.hypot(k.pos.x-p.x-t*dx,k.pos.y-p.y-t*dy)>.9)await act({keys:[],duration_ms:200,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
      else await act({keys:[],duration_ms:200});
    }
    const after=await game.inspect();game.check(id+': native shooting defeats target and awards XP',!entity(after,id)&&after.view.character.experience>xp);return after;
  }
  const victim=family(samples.at(-1)).find(e=>e.kind===0x61c);assert(victim);
  const previous=new Set(family(samples.at(-1)).map(e=>e.id));await defeat(victim.id);
  for(let n=0;n<16&&!family(await game.inspect()).some(e=>e.kind===0x61c&&!previous.has(e.id));n++)await act({keys:[],duration_ms:1000});
  const replenished=await save('replenished');
  game.check('living King replenishes a defeated escort within current caps',entity(replenished,king.id)&&caps(replenished)&&family(replenished).some(e=>e.kind===0x61c&&!previous.has(e.id)));
  await protect();await defeat(king.id);
  const dead=await save('king-defeated');
  await game.act({keys:[],duration_ms:1000},{name:'dwarf-defeated',when:()=>true});
  for(let n=0;n<12;n++)await act({keys:[],duration_ms:1000});
  const end=await save('orphans');
  game.check('dead King cannot replenish or transform',!entity(end,king.id)&&family(end).every(e=>family(dead).some(p=>p.id===e.id)));
  game.check('King XP is not awarded twice',end.view.character.experience===dead.view.character.experience);
  game.check('same prepared native character survives',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
}
