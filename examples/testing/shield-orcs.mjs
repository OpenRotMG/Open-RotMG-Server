import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:831,world:1,equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let queen;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'shield-orc-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  for(const e of map.entities.filter(e=>e.kind===0x900)){
    const king=map.entities.find(m=>m.camp===e.camp&&m.kind===0x901);assert(king);
    for(let dx=-1;dx<=1;dx++)for(let dy=-1;dy<=1;dy++){
      const position={x:Math.floor(king.pos.x)+dx+.5,y:Math.floor(king.pos.y)+dy+.5};
      if(Math.hypot(dx,dy)<1)continue;
      if([-1,0,1].every(x=>[-1,0,1].every(y=>!blocked.has([Math.floor(position.x)+x,Math.floor(position.y)+y].join(','))))){
        queen=e;await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify(map.entities.filter(m=>m.camp===e.camp)));
        return {...spec,position};
      }
    }
  }
  throw Error('No natural HighPlains Shield Orc camp with a clear approach');
}
export async function run(game){
  const samples=[],shots=new Map();let protectedUntil=0,serial=0;
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===queen.camp);
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
  async function step(input,capture){
    let pending=null,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  async function protect(){
    const now=(await game.observe()).game_ms;
    if(now>=protectedUntil){
      const path=join(process.env.QA_RESULTS_DIR,'protection.json');
      await writeFile(path,JSON.stringify({id:'shield-orc-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);protectedUntil=now+60000;
    }
  }
  async function act(input,capture){
    await protect();
    while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await step({keys:[],duration_ms:100});await protect();}
    return step(input,capture);
  }
  try{
  const begin=await save('arrival');
  game.check('natural HighPlains camp has selected initial HP and population',[[0x900,1,1000],[0x901,1,2000],[0x902,2,5000]].every(([k,n,hp])=>family(begin).filter(e=>e.kind===k&&e.hp===hp).length===n));
  await act({keys:[],duration_ms:500},{name:'shield-orcs-arrival',when:()=>true});
  for(let n=0;n<6;n++)await act({keys:[],duration_ms:250});
  await act({keys:[],duration_ms:1500},{name:'shield-orcs-combat',when:()=>true});
  for(const [kind,speed,lifetime] of [[0x900,30,3000],[0x901,60,700],[0x902,60,700]]){
    const ids=new Set(family(begin).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': selected projectile is observed during native combat', [...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
    game.check(kind+': natural actor activates and moves',samples.some(s=>family(s).some(e=>e.kind===kind&&e.activated!==null&&family(begin).some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.2))));
  }
  const king=family(begin).find(e=>e.kind===0x901);assert(king);
  for(let n=0;n<30&&!samples.some(s=>family(s).some(e=>e.id===king.id&&e.hp<1000));n++)await act({keys:[],duration_ms:500,target:{entity:king.id,camera_angle:-Math.PI/4,fire:true}});
  await act({keys:[],duration_ms:2000});
  // Kithio heals to full, unlike our 250-HP reconstruction: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Heal.cs#L40-L47
  game.check('current wounded-King reconstruction heals while Queen lives',samples.some((s,i)=>{const e=family(s).find(e=>e.id===king.id),p=i&&family(samples[i-1]).find(e=>e.id===king.id);return e&&p&&p.hp<1000&&e.hp>p.hp;}));
  for(let n=0;n<30&&family(await game.inspect()).some(e=>e.id===king.id);n++)await act({keys:[],duration_ms:500,target:{entity:king.id,camera_angle:-Math.PI/4,fire:true}});
  const followerDead=await save('king-defeated');
  game.check('ordinary staff defeats King without killing Queen',!family(followerDead).some(e=>e.id===king.id)&&family(followerDead).some(e=>e.id===queen.id));
  await act({keys:[],duration_ms:10000});
  const unreplaced=await save('king-not-replaced');
  game.check('current once-only camp does not replace a dead King',!family(unreplaced).some(e=>e.kind===0x901));
  const xp=unreplaced.view.character.experience;
  for(let n=0;n<60;n++){
    const q=family(await game.inspect()).find(e=>e.id===queen.id);if(!q||q.stage===2)break;
    await act({keys:[],duration_ms:150,target:{entity:queen.id,camera_angle:-Math.PI/4,fire:true}});
  }
  await act({keys:[],duration_ms:1500},{name:'shield-orcs-withdrawn',when:()=>true});
  const dead=await save('queen-withdrawn');
  game.check('wounded Queen telegraphs and withdraws',samples.some(s=>family(s).some(e=>e.id===queen.id&&e.stage===2&&e.hp<500))&&!family(dead).some(e=>e.id===queen.id));
  game.check('Queen emits a ten-shot final ring',samples.some(s=>new Set(s.view.projectiles.filter(p=>p.owner===queen.id&&Math.abs(p.angle/(Math.PI/5)-Math.round(p.angle/(Math.PI/5)))<.001).map(p=>Math.round(p.angle/(Math.PI/5)))).size===10));
  game.check('withdrawal awards no kill XP',dead.view.character.experience===xp);
  const survivorIds=new Set(family(dead).map(e=>e.id)),orphanStart=dead.view.time_ms;
  await act({keys:[],duration_ms:5000});
  const end=await save('survivors'),aftermath=samples.filter(s=>s.view.time_ms>orphanStart+1000);
  game.check('survivors remain and cease firing after command loss',aftermath.length>0&&family(end).length===2&&family(end).every(e=>survivorIds.has(e.id)&&e.stage===4)&&!aftermath.some(s=>s.view.projectiles.some(p=>survivorIds.has(p.owner))));
  game.check('same prepared living Wizard completes the encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  }finally{await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));}
}
