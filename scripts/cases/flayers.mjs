import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
// Ordinary population does not place a Flayer God camp with a clear approach in every draw; pin one that does.
export const realmSeed=1;
const spec={class:0x310,level:20,seed:835,world:1,equipment:[0xa07,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,50,0,50,50,40,75]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let god;
export async function prepare(api){
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  for(const world of [-3,1]){
    const info=await api({op:'world_info',world});assert.equal(info.name,'Realm');const [x,y]=info.spawn_points[0];
    const scout=await api({op:'spawn',spec:{...spec,world,guid:'flayer-scout-'+world,position:{x,y},controller:'direct',profile:null}});
    const state=await api({op:'inspect',actor:scout.result.actor});
    const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
    const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
    await writeFile(join(process.env.QA_RESULTS_DIR,'natural-map-'+world+'.json'),JSON.stringify(map));
    for(const e of map.entities.filter(e=>e.kind===0x649))for(let dx=-9;dx<=9;dx++)for(let dy=-9;dy<=9;dy++){
      const position={x:Math.floor(e.pos.x)+.5+dx,y:Math.floor(e.pos.y)+.5+dy},distance=Math.hypot(position.x-e.pos.x,position.y-e.pos.y);
      if(distance<8||distance>9)continue;
      if(Array.from({length:21},(_,i)=>({x:e.pos.x+(position.x-e.pos.x)*i/20,y:e.pos.y+(position.y-e.pos.y)*i/20})).every(p=>!blocked.has([Math.floor(p.x),Math.floor(p.y)].join(',')))){
        god=e;await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify({world,layout:info.realm_layout,members:map.entities.filter(m=>m.id===e.id||m.parent===e.id)}));return {...spec,world,position};
      }
    }
  }
  throw Error('Neither initial live Realm (-3 and 1) has a natural Flayer God camp with a clear approach');
}
export async function run(game){
  const samples=[],shots=new Map();let serial=0,protectedUntil=0;
  // XML Flayer God MakeMinions children (actors.xml:1428-1429) are owned by parent, not by the natural camp.
  const family=s=>s.diagnostic.entities.filter(e=>e.id===god.id||e.parent===god.id),owned=(s,kind)=>family(s).filter(e=>e.parent===god.id&&e.kind===kind);
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const record=s=>{
    const entities=family(s),owners=new Set(entities.map(e=>e.id));
    samples.push({view:{character:s.view.character},diagnostic:{entities}});
    for(const p of s.view.projectiles)if(owners.has(p.owner))shots.set(p.owner+':'+p.id,p);
  };
  async function step(input,capture){
    let pending=null,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  async function cheat(change){
    const path=join(process.env.QA_RESULTS_DIR,'protection.json');
    await writeFile(path,JSON.stringify({id:'flayer-cheat-'+serial++,command:{op:'cheat',actor:game.actor,change}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);
  }
  let armoredUntil=0;
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil=now+60000;
  }
  async function act(input,capture){
    // Loaded acts can overrun, so renew well before an action could outlast the protection window.
    await protect();while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+5000){
      // Protection cannot overlap, so the idle to expiry overruns into a gap; Ogres beside the camp killed the Priest there.
      const v=await game.observe();if(v.game_ms>=armoredUntil){await cheat({kind:'stat_modifier',index:3,amount:100,duration_ms:60000});armoredUntil=v.game_ms+60000;}
      await cheat({kind:'vitals',hp:v.character.stats.max_hp,mp:v.character.mp});
      await step({keys:[],duration_ms:Math.max(100,protectedUntil-(await game.observe()).game_ms)});await protect();}
    return step(input,capture);
  }
  async function defeat(id){
    const xp=(await game.observe()).character.experience;let lastHp,stalled=0;
    for(let n=0;n<60;n++){
      const s=await game.inspect(),e=family(s).find(e=>e.id===id);if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      stalled=e.hp===lastHp?stalled+1:0;lastHp=e.hp;
      if(stalled===4){await act({keys:[Math.abs(y)>Math.abs(x)?(y>0?'a':'d'):(x>0?'s':'w')],duration_ms:600});stalled=0;continue;}
      if(Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===id&&o.hp>0))await act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:250});
      else await act({keys:[],duration_ms:250,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
      await act({keys:[],duration_ms:250});
    }
    const dead=await save('defeated-'+id);game.check(id+': ordinary native wand defeats actor and earns XP',!family(dead).some(e=>e.id===id)&&dead.view.character.experience>xp);return dead;
  }
  try{
    const begin=await save('arrival');
    // Recovered realm/high.xml:181–182: offspring belong to the XML spawner, not an ordinary population slot.
    game.check('ordinary root is placed before it owns any offspring',begin.diagnostic.entities.some(e=>e.id===god.id&&e.camp===null&&e.parent===null&&e.hp===1400)&&!begin.diagnostic.entities.some(e=>e.parent===god.id));
    await act({keys:[],duration_ms:1000},{name:'flayers-arrival',when:()=>true});
    for(let n=0;n<24;n++)await act({keys:[],duration_ms:500});
    await act({keys:[],duration_ms:1000},{name:'flayers-combat',when:()=>true});
    for(const [kind,speed,lifetime] of [[0x647,50,2700],[0x648,80,1500],[0x649,50,2200],[0x649,70,2200]]){
      const ids=new Set(samples.flatMap(family).filter(e=>e.kind===kind).map(e=>e.id));
      game.check(kind+'/'+speed+': activates and sends selected projectile during native combat',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null))&&[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
    }
    game.check('ordinary zero-damage star applies Quiet and empties mana through Invulnerable',samples.some(s=>(s.view.character.conditions&2)!==0&&s.view.character.mp===0));
    const first=new Map();for(const s of samples)for(const e of family(s))if(!first.has(e.id))first.set(e.id,e.pos);
    for(const kind of [0x647,0x648,0x649])game.check(kind+': actor moves from where it first appeared',samples.some(s=>family(s).some(e=>e.kind===kind&&Math.hypot(e.pos.x-first.get(e.id).x,e.pos.y-first.get(e.id).y)>.2)));
    game.check('God-owned children stay within XML two-Flayer three-Veteran caps',samples.every(s=>[[0x647,2],[0x648,3]].every(([k,n])=>owned(s,k).length<=n)));
    game.check('living God fills its XML two-Flayer three-Veteran caps',[[0x647,2],[0x648,3]].every(([k,n])=>owned(samples.at(-1),k).length===n));
    const victim=owned(await game.inspect(),0x648)[0];assert(victim);
    // The 8s Veteran cooldown can refill during the defeat loop, so compare against the pre-defeat family.
    const old=new Set(family(await game.inspect()).map(e=>e.id));
    await defeat(victim.id);
    // The God only reproduces while a player keeps it awake, and the defeat chase can leave it behind.
    let godPos=samples.map(s=>family(s).find(e=>e.id===god.id)).filter(Boolean).at(-1).pos;
    for(let n=0;n<40;n++){
      const s=await game.inspect();if(owned(s,0x648).some(e=>!old.has(e.id)))break;
      godPos=family(s).find(e=>e.id===god.id)?.pos??godPos;
      const dx=godPos.x-s.view.position.x,dy=godPos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      await act(Math.hypot(dx,dy)>6?{keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:300}:{keys:[],duration_ms:1000});
    }
    const replaced=await save('veteran-replaced');
    game.check('living God replaces a defeated Veteran within its XML cap',family(replaced).some(e=>e.id===god.id)&&owned(replaced,0x648).length<=3&&owned(replaced,0x648).some(e=>!old.has(e.id)));
    await defeat(god.id);
    await act({keys:[],duration_ms:2000});
    const dead=await save('god-defeated'),survivors=family(dead).map(e=>e.id);
    await act({keys:[],duration_ms:10000});
    await act({keys:[],duration_ms:1000},{name:'flayers-defeated',when:()=>true});
    const end=await save('end');
    game.check('God remains dead without duplicate XP',!family(end).some(e=>e.kind===0x649)&&end.view.character.experience===dead.view.character.experience);
    game.check('ordinary followers survive God defeat without further births',family(end).length>0&&family(end).every(e=>survivors.includes(e.id)));
    game.check('same supplied living native Priest completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  }finally{await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));}
}
