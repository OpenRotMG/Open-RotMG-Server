import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x310,level:20,seed:829,world:1,equipment:[0xb3f,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,50,0,50,50,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let god;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'undead-dwarf-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const gods=map.entities.filter(e=>e.kind===0x640);
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-gods.json'),JSON.stringify(gods));
  // The God's XML MakeMinions births its King and escorts where it stands (actors.xml:1262-1266), so approach the God.
  for(const e of gods){
    const x=Math.floor(e.pos.x),y=Math.floor(e.pos.y);
    // Keep the firing lanes open as the God wanders and its escorts move.
    const clear=[-3,-2,-1,0,1,2,3].every(dx=>[-2,-1,0,1,2].every(dy=>!blocked.has([x+dx,y+dy].join(','))));
    if(!clear)continue;
    const position={x:x+2.5,y:y+.5};
    god=e;await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify(map.entities.filter(m=>m.id===god.id||m.parent===god.id)));
    return {...spec,position};
  }
  throw Error('No natural HighPlains God with an open firing lane');
}
export async function run(game){
  const samples=[],shots=new Map();let protectedUntil=0,serial=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  // XML Undead Dwarf God MakeMinions every escort role (actors.xml:1262-1266); they are owned by parent, not by the natural camp.
  const family=s=>s.diagnostic.entities.filter(e=>e.id===god.id||e.parent===god.id),entity=(s,id)=>family(s).find(e=>e.id===id);
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
  async function step(input,capture){
    let pending=null,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const path=join(process.env.QA_RESULTS_DIR,'protection.json');
    await writeFile(path,JSON.stringify({id:'undead-dwarf-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);protectedUntil=now+60000;
  }
  async function act(input,capture){
    await protect();while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await step({keys:[],duration_ms:100});await protect();}
    return step(input,capture);
  }
  // The God makes its whole thirteen-actor roster at once (actors.xml:1262-1266); armor keeps the unprotected exposure survivable.
  const armor=join(process.env.QA_RESULTS_DIR,'armor.json');
  await writeFile(armor,JSON.stringify({id:'undead-dwarf-armor',command:{op:'cheat',actor:game.actor,change:{kind:'stat_modifier',index:3,amount:100,duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),armor]);
  const begin=await save('arrival');
  game.check('ordinary Realm places the selected root without a legacy camp',begin.diagnostic.entities.some(e=>e.id===god.id&&e.camp==null&&e.hp===800));
  await step({keys:[],duration_ms:500},{name:'undead-dwarves-arrival',when:()=>true});
  for(let n=0;n<12&&!samples.some(s=>(s.view.character.conditions&1024)!==0);n++)await step({keys:[],duration_ms:250});
  await save('exposed');
  game.check('ordinary enemy projectiles damage the native Priest',samples.some(s=>s.view.character.hp<begin.view.character.hp));
  game.check('zero-damage Blue Star applies Confused through ordinary native collision',samples.some(s=>(s.view.character.conditions&1024)!==0));
  await protect();
  for(let n=0;n<24;n++)await act({keys:[],duration_ms:500});
  await act({keys:[],duration_ms:500},{name:'undead-dwarves-combat',when:()=>true});
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));
  for(const [kind,speed,lifetime] of [[0x640,70,1700],[0x63e,100,600],[0x63f,70,1700],[0x63a,60,1200],[0x63b,120,400],[0x63c,50,2400]]){
    const ids=new Set(samples.flatMap(family).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': natural actor activates and fires the selected projectile',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null))&&[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
  }
  const roster=[[0x640,1,800],[0x63e,2,400],[0x63f,1,250],[0x63a,3,250],[0x63b,3,260],[0x63c,3,210]],caps=s=>roster.every(([k,n])=>family(s).filter(e=>e.kind===k).length<=n);
  game.check('XML population caps hold',samples.every(caps));
  const first=new Map();for(const s of samples)for(const e of family(s))if(!first.has(e.id))first.set(e.id,e);
  game.check('XML children have selected HP',[...first.values()].every(e=>e.hp===roster.find(([k])=>k===e.kind)[2]));
  game.check('living God fills its XML roster',roster.every(([k,n])=>family(samples.at(-1)).filter(e=>e.kind===k).length===n));
  async function defeat(id){
    const before=await game.inspect();
    for(let n=0;n<140;n++){
      const s=await game.inspect(),target=entity(s,id);if(!target)break;
      const dx=target.pos.x-s.view.position.x,dy=target.pos.y-s.view.position.y;
      // The native client remaps movement under Blue Star Confused; unadjusted pursuit walks away from the God.
      const direction=(s.view.character.conditions&1024)!==0?-1:1;
      const x=direction*(dx-dy)/Math.SQRT2,y=direction*(dx+dy)/Math.SQRT2;
      if(Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===id)){
        await act({keys:[Math.abs(x)>.18?(x>0?'d':'a'):null,Math.abs(y)>.18?(y>0?'s':'w'):null].filter(Boolean),duration_ms:150});
      }else await act({keys:[],duration_ms:150,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    }
    const after=await save('defeat-'+id);
    game.check(id+': ordinary native wand defeats actor and earns XP',!entity(after,id)&&after.view.character.experience>before.view.character.experience);
    return after;
  }
  // The selected MultiHit Crystal Wand reaches the God through the replenishing front line.
  const king=family(await game.inspect()).find(e=>e.kind===0x63e);assert(king);
  // The God's 10s King cooldown has long elapsed, so it can refill within the defeat tick; compare with the pre-defeat family.
  const old=new Set(family(await game.inspect()).map(e=>e.id));await defeat(king.id);
  for(let n=0;n<15&&!family(await game.inspect()).some(e=>e.kind===0x63e&&!old.has(e.id));n++)await act({keys:[],duration_ms:1000});
  const replaced=await save('king-replaced');
  game.check('living God replaces a defeated King within current cap',entity(replaced,god.id)&&family(replaced).filter(e=>e.kind===0x63e).length<=2&&family(replaced).some(e=>e.kind===0x63e&&!old.has(e.id)));
  await defeat(god.id);const dead=await save('god-defeated');
  await act({keys:[],duration_ms:500},{name:'undead-dwarves-defeated',when:()=>true});
  const survivors=family(dead).map(e=>e.id);
  for(let n=0;n<12;n++)await act({keys:[],duration_ms:1000});
  const end=await save('survivors');
  game.check('God defeat preserves ordinary followers',family(end).some(e=>survivors.includes(e.id)));
  game.check('God stays dead and cannot duplicate its XP',!entity(end,god.id)&&end.view.character.experience===dead.view.character.experience);
  game.check('same supplied living native Priest completes the encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  for(const s of samples)for(const e of family(s))if(!first.has(e.id))first.set(e.id,e);
  for(const kind of [0x640,0x63e,0x63f,0x63a,0x63b,0x63c])game.check(kind+': actor moves from where it first appeared',samples.some(s=>family(s).some(e=>e.kind===kind&&Math.hypot(e.pos.x-first.get(e.id).pos.x,e.pos.y-first.get(e.id).pos.y)>.2)));
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));
}
