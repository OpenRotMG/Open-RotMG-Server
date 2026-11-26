import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:831,world:1,equipment:[0xb08,0xb24,-1,-1],backpack:[0xa97,...Array(7).fill(-1)],stats:[670,385,75,0,50,75,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let queen;
export async function prepare(api){
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  // Some sampled layouts place no HighPlains Shield Orcs, so search both initial live Realms (flayers precedent).
  for(const world of [1,-3]){
    const info=await api({op:'world_info',world});assert.equal(info.name,'Realm');const [x,y]=info.spawn_points[0];
    const scout=await api({op:'spawn',spec:{...spec,world,guid:'shield-orc-scout-'+world,position:{x,y},controller:'direct',profile:null}});
    const state=await api({op:'inspect',actor:scout.result.actor});
    const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
    const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
    // The Key's XML MakeMinions births its Flooder where the Key stands (actors.xml:2654), so approach the Key.
    for(const e of map.entities.filter(e=>e.kind===0x900)){
      for(let dx=-1;dx<=1;dx++)for(let dy=-1;dy<=1;dy++){
        const position={x:Math.floor(e.pos.x)+dx+.5,y:Math.floor(e.pos.y)+dy+.5};
        if(Math.hypot(dx,dy)<1)continue;
        if(!blocked.has([Math.floor(position.x),Math.floor(position.y)].join(','))){
          queen=e;await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify({world,layout:info.realm_layout,members:map.entities.filter(m=>m.id===e.id||m.parent===e.id)}));
          return {...spec,world,position};
        }
      }
    }
  }
  throw Error('Neither initial live Realm has a Shield Orc Key with a walkable adjacent approach');
}
export async function run(game){
  const samples=[],shots=new Map();let protectedUntil=0,serial=0,pollMs=100;
  // XML Key MakeMinions one Flooder and one Shield (actors.xml:2654-2656); both are owned by the Key, not the natural camp.
  const family=s=>s.diagnostic.entities.filter(e=>e.id===queen.id||e.parent===queen.id);
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
  async function step(input,capture){
    let pending=null,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},pollMs);
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
  // Recovered realm/high.xml:596–598: offspring belong to the XML spawner, not an ordinary population slot.
  game.check('ordinary root is placed before it owns any offspring',begin.diagnostic.entities.some(e=>e.id===queen.id&&e.camp===null&&e.parent===null&&e.hp===1000)&&!begin.diagnostic.entities.some(e=>e.parent===queen.id));
  await act({keys:[],duration_ms:500},{name:'shield-orcs-arrival',when:()=>true});
  const activated=await save('activated');
  game.check('activated Key makes one selected-HP Flooder and one Shield',[[0x901,2000],[0x902,5000]].every(([k,hp])=>family(activated).filter(e=>e.parent===queen.id&&e.kind===k).map(e=>e.hp).join()===String(hp)));
  for(let n=0;n<6;n++)await act({keys:[],duration_ms:250});
  await act({keys:[],duration_ms:1500},{name:'shield-orcs-combat',when:()=>true});
  // The Key's Attack1 Order makes its listeners shoot (actors.xml:2663-2668); the Key fires only when it Explodes.
  for(const [kind,speed,lifetime] of [[0x900],[0x901,60,700],[0x902,60,700]]){
    const ids=new Set(family(activated).filter(e=>e.kind===kind).map(e=>e.id));
    if(speed)game.check(kind+': selected projectile is observed during native combat', [...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
    // The Key and Shield Orbit the Flooder (actors.xml:2662,2718); the Flooder only FollowOrders, so it holds as their anchor.
    game.check(kind+(kind===0x901?': anchor activates':': actor activates and orbits'),samples.some(s=>family(s).some(e=>e.kind===kind&&e.activated!==null&&(kind===0x901||family(activated).some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.2)))));
  }
  const king=family(activated).find(e=>e.kind===0x901);assert(king);
  for(let n=0;n<30&&!samples.some(s=>family(s).some(e=>e.id===king.id&&e.hp<1000));n++)await act({keys:[],duration_ms:500,target:{entity:king.id,camera_angle:-Math.PI/4,fire:true}});
  await act({keys:[],duration_ms:2000});
  // Below half HP the ordered Attack1 listener switches to Heal and heals the Shield Orcs group (actors.xml:2665-2671),
  // so the Flooder is not burst down here; the Key's Explode order ends it below.
  game.check('wounded Flooder below half HP is healed by the ordered group Heal',samples.some((s,i)=>{const e=family(s).find(e=>e.id===king.id),p=i&&family(samples[i-1]).find(e=>e.id===king.id);return e&&p&&p.hp<1000&&e.hp>p.hp;}));
  await act({keys:[],duration_ms:500,drag:{from:{x:634,y:428},to:{x:634,y:385}}});
  game.check('native inventory equips the supplied low-damage staff for withdrawal',(await game.observe()).character.items[0]===0xa97);
  // Below half HP the Key flashes for 0.3s, then offers an Explode order and Explodes (actors.xml:2674-2689).
  const states=(s,id)=>(s.diagnostic.xml[id]?.states??[]).map(x=>x.name),xp=(await game.observe()).character.experience;
  for(let n=0;n<120;n++){
    const s=await game.inspect(),q=family(s).find(e=>e.id===queen.id);if(!q||states(s,q.id).includes('FlashBeforeExplode'))break;
    // Dense sampling only near the 0.3s flash keeps the evidence file bounded.
    pollMs=q.hp<700?50:100;
    const dx=q.pos.x-s.view.position.x,dy=q.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
    if(Math.hypot(dx,dy)>4){await act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:200});continue;}
    await act({keys:[],duration_ms:50,target:{entity:queen.id,camera_angle:-Math.PI/4,fire:true}});
    await act({keys:[],duration_ms:500});
  }
  await act({keys:[],duration_ms:1500},{name:'shield-orcs-exploded',when:()=>true});
  const dead=await save('queen-exploded');
  game.check('wounded Key flashes below half HP and Explodes',samples.some(s=>family(s).some(e=>e.id===queen.id&&e.hp<500&&states(s,e.id).includes('FlashBeforeExplode')))&&!family(dead).some(e=>e.id===queen.id));
  // A terminal owner can vanish between inspections. Correlate the projectile send with the actual native SHOOT acknowledgement.
  const log=await readFile(join(root(),'server.log'),'utf8');
  const facts=log.split('\n').filter(line=>line.startsWith('{')&&line.includes('game audit fact')).flatMap(line=>JSON.parse(JSON.parse(line).fields.record).data.facts??[]);
  const volley=facts.find(f=>f.event==='volley_attempt'&&f.values.owner===queen.id&&f.values.spec.speed===30&&f.values.spec.lifetime===3000&&f.values.recipients.some(([, ,player])=>player===begin.diagnostic.player));
  const ack=volley&&facts.find(f=>f.event==='shoot_ack'&&f.identity?.[2]===begin.diagnostic.player&&volley.values.shots.every(([id,born])=>f.values.generations.some(([owner,b,t])=>owner===queen.id&&b===id&&t===born)));
  await writeFile(join(process.env.QA_RESULTS_DIR,'key-explode.json'),JSON.stringify({volley,ack}));
  game.check('Key Explode sends its selected projectile and the native client acknowledges it',!!ack);
  pollMs=100;
  await act({keys:[],duration_ms:5000});
  const end=await save('after-explode');
  // FollowOrders queries a still-living issuer on the listener's own update (Release 7.0 behavior.cc:1460-1489).
  game.check('retired Key grants no XP and cannot deliver its final order after death',end.view.character.experience===xp&&family(activated).filter(e=>e.parent===queen.id).every(child=>family(end).some(e=>e.id===child.id&&e.hp>0)));
  game.check('same prepared living Wizard completes the encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  }finally{await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples:samples.length,shots:[...shots.values()]}));}
}
