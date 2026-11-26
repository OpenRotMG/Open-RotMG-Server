import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const selected=Number(process.env.GOLEM_KIND);
assert([0x62f,0x631,0x62d,0x62e,0x632,0x633].includes(selected),'Set GOLEM_KIND to one of 0x62f/0x631/0x62d/0x62e/0x632/0x633');
const spec={class:0x30e,level:20,seed:838,world:1,equipment:[0xb08,-1,-1,-1],backpack:[selected===0x633?0xb10:-1,...Array(7).fill(-1)],stats:[670,385,75,0,50,75,40,60]};
const output=name=>join(process.env.QA_RESULTS_DIR,name);
const native=name=>output('native/'+name);
let camps,blocked;
const dist=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
const open=p=>!blocked.has([Math.floor(p.x),Math.floor(p.y)].join(','));
export async function prepare(api){
  const report=JSON.parse(await readFile(native('result.json'),'utf8'));
  const token=(await readFile(native('state/control.token'),'utf8')).trim();
  for(const world of [1,-3]){
    const info=await api({op:'world_info',world}),[x,y]=info.spawn_points[0];
    const scout=await api({op:'spawn',spec:{...spec,world,guid:'golem-scout-'+world,position:{x,y},controller:'direct',profile:null}});
    const state=await api({op:'inspect',actor:scout.result.actor});
    const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
    const map=await response.json();blocked=new Set(map.blocked.map(p=>p.join(',')));
    // Neighbouring leaders' XML children wander into the staff's two shots, so prefer the most isolated golem.
    const others=map.entities.filter(p=>p.hp>0),clearance=new Map(map.entities.filter(e=>e.kind===selected).map(e=>[e,Math.min(...others.filter(p=>p.camp!==e.camp).map(p=>dist(p.pos,e.pos)))]));
    for(const root of [...clearance.keys()].sort((a,b)=>clearance.get(b)-clearance.get(a))){
      for(let dx=-3;dx<=3;dx++)for(let dy=-3;dy<=3;dy++){
        if(Math.hypot(dx,dy)<2||Math.hypot(dx,dy)>3)continue;
        const position={x:Math.floor(root.pos.x)+dx+.5,y:Math.floor(root.pos.y)+dy+.5};
        if(!Array.from({length:7},(_,i)=>i-3).every(x=>Array.from({length:7},(_,i)=>i-3).every(y=>open({x:position.x+x,y:position.y+y}))))continue;
        camps=[{root,position,members:map.entities.filter(e=>e.camp===root.camp)}];
        await writeFile(output('natural-camps.json'),JSON.stringify({world,layout:info.realm_layout,camps}));
        return {...spec,world,position};
      }
    }
  }
  throw Error('Neither initial Realm has a walkable natural camp for '+selected.toString(16));
}
export async function run(game){
  const samples=[],shots=new Map(),encounters=[];let serial=0,protectedUntil=0;
  const command=async change=>{
    const path=output('mutation-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'golem-'+serial++,command:{op:'cheat',actor:game.actor,change}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),native('native.json'),path]);
  };
  const family=(s,camp)=>s.diagnostic.entities.filter(e=>e.camp===camp||camps.some(c=>c.root.camp===camp&&e.parent===c.root.id));
  async function step(input,capture){
    let pending,error;
    const record=s=>{samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  async function act(input,capture){
    async function protect(){if((await game.observe()).game_ms>=protectedUntil){await command({kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil=(await game.observe()).game_ms+60000;}}
    await protect();
    while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await step({keys:[],duration_ms:500});await protect();}
    return step(input,capture);
  }
  try{
    for(const {root,members} of camps){
      const tag=root.kind.toString(16),camp=root.camp,start=samples.length;
      const arrival=await game.inspect();
      // XML golems make their colored satellite and one Gray kind (actors.xml:890-1051); the camp places only the other two Grays.
      const placed=members.filter(e=>e.kind>=0x6d2&&e.kind<=0x6d7);
      game.check(tag+': natural root and its two camp Gray Satellites admitted',family(arrival,camp).some(e=>e.id===root.id&&e.hp===220)&&placed.length===2&&placed.every(e=>e.kind>=0x6d5)&&placed[0].kind!==placed[1].kind);
      await act({keys:[],duration_ms:2000});
      await act({keys:[],duration_ms:4500},{name:'golem-'+tag,when:view=>view.game_ms>=arrival.view.time_ms+4000});
      const speeds={0x62f:[55],0x631:[55],0x62d:[75],0x62e:[10],0x632:[45,60],0x633:[120,70]};
      // Darkness Golem fires projectile 1 only in back_up, about 8s after it notices the player (actors.xml:1067-1070).
      for(let n=0;n<10&&!speeds[root.kind].every(sp=>[...shots.values()].some(p=>p.owner===root.id&&p.speed===sp));n++)await act({keys:[],duration_ms:1000});
      const combat=samples.slice(start);
      game.check(tag+': root activates and moves',combat.some(s=>family(s,camp).some(e=>e.id===root.id&&e.activated!==null&&Math.hypot(e.pos.x-root.pos.x,e.pos.y-root.pos.y)>.2)));
      for(const speed of speeds[root.kind])game.check(tag+': native root projectile speed '+speed,[...shots.values()].some(p=>p.owner===root.id&&p.speed===speed));
      const grays=new Map(combat.flatMap(s=>family(s,camp)).filter(e=>e.kind>=0x6d5&&e.kind<=0x6d7).map(e=>[e.id,e.kind]));
      for(const kind of [0x6d5,0x6d6,0x6d7])game.check(tag+': gray '+kind.toString(16)+' fires selected missile',[...shots.values()].some(p=>grays.get(p.owner)===kind&&p.speed===65&&p.lifetime_ms===2500));
      // XML golems make their own satellites (actors.xml:952-953), which orbit by protectee name at radius 4 and 1.5
      // (actors.xml:2397,2431) and Decay after 16s (actors.xml:2398).
      const owned=s=>family(s,camp).filter(e=>e.parent===root.id&&e.kind>=0x6d2&&e.kind<=0x6d7),near=(a,b,r)=>a&&b&&Math.abs(Math.hypot(a.pos.x-b.pos.x,a.pos.y-b.pos.y)-r)<.3;
      const hub=combat.flatMap(owned).find(e=>e.kind<=0x6d4);assert(hub,'Root made no colored satellite');
      game.check(tag+': root makes the Gray kind its camp lacks',combat.flatMap(owned).some(e=>e.kind>=0x6d5&&!placed.some(p=>p.kind===e.kind)));
      game.check(tag+': owned colored satellite orbits its own root',combat.some(s=>near(family(s,camp).find(e=>e.id===hub.id),family(s,camp).find(e=>e.id===root.id),4)));
      game.check(tag+': owned gray satellites orbit the colored satellite',combat.some(s=>{const h=family(s,camp).find(e=>e.id===hub.id);return owned(s).some(e=>e.kind>=0x6d5&&near(e,h,1.5));}));
      // 1000 HP must fall inside the 16s Decay window, so wait for the root's next fresh batch.
      // A native effusion can coexist with the single prepared Attack modifier.
      if(root.kind===0x633){
        const dex=(await game.observe()).character.stats.dexterity;
        await act({keys:['1'],duration_ms:150});
        game.check('Dark satellite fixture consumes one Dexterity effusion',(await game.observe()).character.stats.dexterity===dex+30);
      }
      const keysToward=(p,e)=>{const dx=e.x-p.x,dy=e.y-p.y,x=dx-dy,y=dx+dy;return [Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean);};
      const clear=(p,e)=>Array.from({length:Math.ceil(dist(p,e)*10)},(_,i)=>{const t=i/Math.ceil(dist(p,e)*10);return open({x:p.x+t*(e.x-p.x),y:p.y+t*(e.y-p.y)});}).every(Boolean);
      const approach=(p,e)=>{
        const steps=[e,...Array.from({length:8},(_,i)=>({x:p.x+Math.cos(i*Math.PI/4),y:p.y+Math.sin(i*Math.PI/4)}))];
        const step=steps.filter(q=>dist(q,p)>.1&&clear(p,q)).sort((a,b)=>dist(a,e)-dist(b,e))[0];
        return {keys:step?keysToward(p,step):[],duration_ms:250};
      };
      // The backing Golem can leave the starting lane while its first satellite ages out.
      let target;
      for(let n=0;n<64&&!target;n++){
        const s=await game.inspect(),p=s.view.position,e=family(s,camp).find(e=>e.id===root.id);
        assert(e,'Living Golem required for a renewed satellite');
        if(dist(p,e.pos)>3){await act(approach(p,e.pos));continue;}
        target=owned(s).find(e=>e.kind<=0x6d4&&e.activated!==null&&s.view.time_ms-e.activated<3000);
        if(!target)await act({keys:[],duration_ms:750});
      }
      assert(target,'Root did not renew its nearby colored satellite');
      // Temporary attack fixture (cube-god precedent) so an orbiting 1000 HP satellite falls inside its Decay window.
      await command({kind:'stat_modifier',index:2,amount:100,duration_ms:30000});
      let reached=false;
      const xp=(await game.observe()).character.experience;
      for(let n=0;n<65;n++){
        const s=await game.inspect(),e=family(s,camp).find(e=>e.id===target.id);if(!e)break;
        const p=s.view.position;
        if(dist(p,e.pos)>3||!clear(p,e.pos)||!s.view.objects.some(o=>o.id===e.id)){await act(approach(p,e.pos));continue;}
        reached=true;
        const dx=e.pos.x-p.x,dy=e.pos.y-p.y;
        const inLine=s.diagnostic.entities.some(o=>o.hp>0&&o.id!==e.id&&(t=>t>=0&&t<=1&&Math.hypot(o.pos.x-p.x-t*dx,o.pos.y-p.y-t*dy)<.55)(((o.pos.x-p.x)*dx+(o.pos.y-p.y)*dy)/(dx*dx+dy*dy)));
        await act(inLine?{keys:[],duration_ms:100}:{keys:[],duration_ms:250,target:{entity:e.id,camera_angle:-Math.PI/4,fire:true}});
      }
      await act({keys:[],duration_ms:300});const satelliteDead=await game.inspect();
      game.check(tag+': ordinary native movement reaches a fresh satellite',reached);
      // Ten XP, and an exact actor receipt (natural Decay does not give XP).
      // A second missile can hit a neighboring actor; read the selected victim's own durable XP step.
      const {state}=JSON.parse(await readFile(native('native.json'),'utf8'));
      const awards=JSON.parse(execFileSync('python3',['-c',`
import json,sqlite3,sys
with sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True) as db:
    rows=db.execute("SELECT payload FROM world_events WHERE kind='terminal' AND status=2")
    events=[json.loads(row[0])['payload']['data'] for row in rows]
    print(json.dumps(sorted([[c['kills'],c['exp'],e['defeat']['actor']] for e in events for c,_ in e['actions'].get('progress',[]) if c['id']==int(sys.argv[2])])))
`,join(state,'game.db'),String(satelliteDead.view.session.character)],{encoding:'utf8'}));
      const awarded=awards.filter(([,exp])=>exp>xp),index=awarded.findIndex(([, ,id])=>id===target.id);
      await writeFile(output('satellite-xp.json'),JSON.stringify({id:target.id,before:xp,awarded,after:satelliteDead.view.character.experience}));
      game.check(tag+': native satellite defeat awards ten XP',!family(satelliteDead,camp).some(e=>e.id===target.id)&&
        index>=0&&awarded.filter(([, ,id])=>id===target.id).length===1&&awarded[index][1]-(awarded[index-1]?.[1]??xp)===10&&
        awarded.at(-1)[1]===satelliteDead.view.character.experience);
      for(let n=0;n<52;n++){
        const s=await game.inspect(),e=family(s,camp).find(e=>e.id===root.id);if(!e)break;
        const p=s.view.position;
        await act(dist(p,e.pos)>3||!clear(p,e.pos)||!s.view.objects.some(o=>o.id===e.id)?approach(p,e.pos):{keys:[],duration_ms:300,target:{entity:e.id,camera_angle:-Math.PI/4,fire:true}});
      }
      const defeated=await game.inspect();
      game.check(tag+': ordinary native fire defeats root',!family(defeated,camp).some(e=>e.id===root.id));
      const survivors=owned(defeated);
      game.check(tag+': owned satellites survive root death',survivors.length>0);
      const cleanupXp=defeated.view.character.experience;
      for(let n=0;n<2;n++)await act({keys:[],duration_ms:10000});
      const end=await game.inspect();
      game.check(tag+': orphan satellites Decay without kill XP',!family(end,camp).some(e=>survivors.some(p=>p.id===e.id))&&end.view.character.experience===cleanupXp);
      game.check(tag+': same prepared living Wizard completes encounter',end.view.lifecycle.alive&&end.view.session.character===arrival.view.session.character);
      encounters.push({kind:root.kind,arrival,defeated,satelliteDead,end});
    }
  }finally{await writeFile(output('combat.json'),JSON.stringify({encounters,samples,shots:[...shots.values()]}));}
}
