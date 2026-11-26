import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:5,seed:12351,world:1,equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,10,0,30,10,0,0]};
let leader;
export async function prepare(api){
  const root=join(process.env.QA_RESULTS_DIR,'native');
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  for(const world of [1,-3]){
    const info=await api({op:'world_info',world}),[x,y]=info.spawn_points[0];
    const scout=await api({op:'spawn',spec:{...spec,world,guid:'werewolf-scout-'+world,position:{x,y},controller:'direct',profile:null}});
    const state=await api({op:'inspect',actor:scout.result.actor});
    const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
    const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
    const clear=(x,y)=>!blocked.has([Math.floor(x),Math.floor(y)].join(','));
    for(const e of map.entities.filter(e=>e.kind===0x623))for(const [dx,dy] of [[-1,0],[0,-1],[1,0],[0,1]]){
      const position={x:e.pos.x+3*dx,y:e.pos.y+3*dy};
      if(![0,1,2,3].every(n=>clear(e.pos.x+n*dx,e.pos.y+n*dy))||![-1,0,1].every(i=>[-1,0,1].every(j=>clear(position.x+i,position.y+j))))continue;
      leader=e;await writeFile(join(process.env.QA_RESULTS_DIR,'natural-family.json'),JSON.stringify({world,layout:info.realm_layout,leader,position}));
      return {...spec,world,position};
    }
  }
  assert.fail('Neither initial Realm has a natural Desert Werewolf with a clear approach');
}
export async function run(game){
  let serial=0,protectedUntil=0;
  async function act(input,capture){
    while(protectedUntil&&(await game.observe()).game_ms<protectedUntil&&(await game.observe()).game_ms+input.duration_ms+1500>=protectedUntil)await game.act({keys:[],duration_ms:100});
    const now=(await game.observe()).game_ms;
    if(now>=protectedUntil){
      const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
      await writeFile(path,JSON.stringify({id:'werewolf-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);protectedUntil=now+60000;
    }
    return game.act(input,capture);
  }
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  // XML Werewolf MakeMinions draws eight Wargs from its group (actors.xml:762); they are owned by parent, not by the natural camp.
  const family=s=>s.diagnostic.entities.filter(e=>(e.id===leader.id||e.parent===leader.id)&&[0x620,0x622].includes(e.kind));
  const begin=await save('arrival');
  game.check('natural MidSand leader has selected HP and size',entity(begin,leader.id).hp===320&&entity(begin,leader.id).size===135);
  // Recovered realm/mid.xml:90: offspring belong to the XML spawner, not an ordinary population slot.
  game.check('ordinary root is placed before it owns any offspring',begin.diagnostic.entities.some(e=>e.id===leader.id&&e.camp===null&&e.parent===null)&&!begin.diagnostic.entities.some(e=>e.parent===leader.id));
  // The eight XML Wargs arrive at once (actors.xml:762), so stop the unprotected exposure at the first hit.
  for(let n=0;n<20&&(await game.observe()).character.hp>=begin.view.character.hp;n++)await game.act({keys:[],duration_ms:100,target:{entity:leader.id,camera_angle:-Math.PI/4,fire:false}});
  const exposed=await save('exposed');
  game.check('unprotected native Wizard takes encounter damage',exposed.view.character.hp<begin.view.character.hp);
  game.check('level-five native player receives a visible Werewolf quest',entity(exposed,exposed.view.quest)?.kind===0x623);
  game.check('activated Werewolf makes its eight XML Wargs',family(exposed).length===8&&family(exposed).every(e=>e.parent===leader.id));
  const samples=[];
  for(let n=0;n<40;n++){await act({keys:[],duration_ms:500},n===39?{name:'werewolf-unharmed',when:()=>true}:undefined);samples.push(await game.inspect());}
  // The group draw can leave a single Warg of one kind (actors.xml:762); keep observing until each drawn kind has fired.
  const fired=kind=>samples.some(s=>s.view.projectiles.some(p=>p.speed===60&&p.lifetime_ms===(kind===0x620?600:2000)&&family(s).some(e=>e.id===p.owner&&e.kind===kind)));
  for(let n=0;n<20&&![0x620,0x622].every(fired);n++){await act({keys:[],duration_ms:500});samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify(samples));
  game.check('Werewolf activates and wanders',samples.some(s=>{const e=entity(s,leader.id);return e.activated!==null&&Math.hypot(e.pos.x-leader.pos.x,e.pos.y-leader.pos.y)>.2;}));
  game.check('living Werewolf keeps its XML eight-Warg cap',family(samples.at(-1)).length===8&&samples.every(s=>family(s).length<=8));
  const shots=samples.flatMap(s=>s.view.projectiles),ids=kind=>new Set(samples.flatMap(family).filter(e=>e.kind===kind).map(e=>e.id));
  game.check('unharmed Werewolf fires selected speed/lifetime',shots.some(p=>p.owner===leader.id&&p.speed===60&&p.lifetime_ms===2000));
  for(const [kind,lifetime] of [[0x620,600],[0x622,2000]])game.check(kind+': Wargs move and fire selected speed/lifetime',samples.some(s=>family(s).some(e=>e.kind===kind&&family(exposed).some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.1)))&&shots.some(p=>ids(kind).has(p.owner)&&p.speed===60&&p.lifetime_ms===lifetime));
  // A key direction can be pinned at the arrival tile (observed with the XML Warg pack), so fall back to the opposite pair.
  let origin,right,down,ux,uy,vx,vy,det=0;
  for(const [h,v,sign] of [['d','s',1],['a','w',-1]]){
    origin=(await game.observe()).position;
    await act({keys:[h],duration_ms:100});right=(await game.observe()).position;
    await act({keys:[v],duration_ms:100});down=(await game.observe()).position;
    [ux,uy,vx,vy]=[right.x-origin.x,right.y-origin.y,down.x-right.x,down.y-right.y].map(n=>n*sign);det=ux*vy-uy*vx;
    if(Math.abs(det)>.05)break;
  }
  game.check('native movement supplies a non-collinear observed input basis',Math.abs(det)>.05);
  const camera=det>0?Math.atan2(uy,ux):Math.atan2(ux,-uy);
  await writeFile(join(process.env.QA_RESULTS_DIR,'movement-basis.json'),JSON.stringify({origin,right,down,det,camera}));
  async function shoot(id,stop){
    for(let n=0;n<160;n++){
      const s=await game.inspect(),e=entity(s,id);if(!e||stop?.(e))return;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,a=camera,x=(dx*vy-dy*vx)/det,y=(ux*dy-uy*dx)/det;
      const approach=Math.hypot(dx,dy)>5;
      const keys=approach?[...(Math.abs(x)>Math.abs(y)*.5?[x>0?'d':'a']:[]),...(Math.abs(y)>Math.abs(x)*.5?[y>0?'s':'w']:[])]:[];
      await act(approach?{keys,duration_ms:250}:{keys:[],duration_ms:250,target:{entity:id,camera_angle:a,fire:true}});
    }
    assert.fail('Native shots failed to reach target outcome: '+id);
  }
  const victim=family(samples.at(-1))[0],beforeWarg=(await game.observe()).character.experience;
  await shoot(victim.id);await act({keys:[],duration_ms:500});
  game.check('native shooting defeats Warg and earns XP',!entity(await game.inspect(),victim.id)&&(await game.observe()).character.experience>beforeWarg);
  for(let n=0;n<22;n++)await act({keys:[],duration_ms:500});
  const replaced=await save('replenished');
  game.check('living Werewolf replenishes a defeated Warg within its cap',family(replaced).length===8&&family(replaced).some(e=>!ids(0x620).has(e.id)&&!ids(0x622).has(e.id)));
  // XML enrages below 75% HP: ChangeSize rate 20 up to 170 and projectile 1 (actors.xml:767-775).
  await shoot(leader.id,e=>e.hp<240);
  await act({keys:[],duration_ms:600},{name:'werewolf-enraged',when:()=>true});
  const rage=await save('enraged');
  game.check('native damage enters the XML enraged state and grows',entity(rage,leader.id)?.hp<240&&rage.diagnostic.xml[leader.id]?.states.some(x=>x.name==='enraged')&&entity(rage,leader.id).size>135);
  const enraged=[];
  for(let n=0;n<8;n++){await act({keys:[],duration_ms:250});enraged.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'rage-attacks.json'),JSON.stringify(enraged));
  game.check('enraged Werewolf fires faster selected projectile',enraged.flatMap(s=>s.view.projectiles).some(p=>p.owner===leader.id&&p.speed===65&&p.lifetime_ms===2000));
  const xp=(await game.observe()).character.experience;
  await shoot(leader.id);await act({keys:[],duration_ms:600},{name:'werewolf-defeated',when:()=>true});
  const dead=await save('defeated');
  game.check('native shooting defeats Werewolf and commits eligible XP',!entity(dead,leader.id)&&dead.view.character.experience>=xp+52);
  await act({keys:[],duration_ms:6000});await act({keys:[],duration_ms:6000});
  const end=await save('finished');
  game.check('dead Werewolf cannot replenish Wargs',!entity(end,leader.id)&&family(end).every(e=>family(dead).some(p=>p.id===e.id)));
  game.check('defeat reward is not repeated',end.view.character.experience===dead.view.character.experience);
  game.check('same prepared living character completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
}
