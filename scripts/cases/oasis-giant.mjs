import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {realmSiteTerrain} from './realm-site-terrain.mjs';
// Procedural (FSoD) Oasis sites put the chest under the boss; authored classic sites use their recorded chest cell.
// The authored 0–4 roll can be zero in both Realms (game.xml NamedSite for Oasis Giant).
export const realmSeed=105;
const out=process.env.QA_RESULTS_DIR;
let placement;
export async function prepare(api){
  const events=(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  const ends=events.flatMap((e,i)=>e.message==='Realm simulation populated'?[i]:[]);assert(ends.length===2,'Two initial Realm populations required');
  for(const [index,id] of [-3,1].entries()){
    const roots=events.slice(index===0?0:ends[index-1]+1,ends[index]).filter(e=>e.message==='linked Realm site placed'&&e.kind===0x678);
    if(!roots.length)continue;
    const world=await api({op:'world_info',world:id}),root=roots[0];assert(world.realm_layout===root.layout,'Placement log must match the actual world');
    const catalogue=JSON.parse(await readFile(new URL('../../data/realm-sites.json',import.meta.url),'utf8'));
    const site=catalogue.find(e=>e.layout===root.layout)?.sites.find(e=>e.kind===root.kind&&e.spawn[0]===root.x&&e.spawn[1]===root.y);
    placement={root,chest:site?.chest??[root.x,root.y]};
    if(!site){
      const terrain=await realmSiteTerrain(world.realm_layout);
      assert([2,5].includes(terrain(root.x,root.y)),'Procedural Oasis must be in MidSand or HighSand');
    }
    await writeFile(join(out,'placement.json'),JSON.stringify({world,...placement}));
    return {class:0x307,level:20,seed:797,world:id,position:{x:root.x+6,y:root.y},equipment:[0xc02,-1,0xaf9,-1],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50]};
  }
  throw Error('Neither initial Realm contains an ordinarily placed Oasis');
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const save=async name=>{const s=await game.inspect();await writeFile(join(out,name+'.json'),JSON.stringify(s));return s;};
  let protectedUntil=0;
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));protectedUntil=now+60000;
  }
  async function idle(ms){while(ms>0){await protect();const step=Math.min(ms,5000,protectedUntil-(await game.observe()).game_ms);await game.act({keys:[],duration_ms:step});ms-=step;}}
  const initial=await save('initial'),boss=initial.diagnostic.entities.find(e=>e.id===placement.root.entity);assert(boss);
  // XML MakeMinions (groupId Oasis) owns children by parent, not the legacy quest encounter field.
  const children=s=>s.diagnostic.entities.filter(e=>e.parent===boss.id&&e.kind>=0x679&&e.kind<=0x67c);
  const chest=initial.diagnostic.entities.find(e=>e.kind===0x501&&e.pos.x===placement.chest[0]&&e.pos.y===placement.chest[1]);assert(chest,'Natural Oasis chest required');
  await idle(3000);await game.act({keys:[],duration_ms:1000},{name:'oasis-entry',when:()=>true});
  const trace=[];let pending=null,sampleError;
  const sample=async()=>{const s=await game.inspect();trace.push({view:s.view,children:children(s),boss:s.diagnostic.entities.find(e=>e.id===boss.id)});};
  const timer=setInterval(()=>{if(!pending)pending=sample().catch(e=>{sampleError=e;}).finally(()=>{pending=null;});},100);
  try{await game.act({keys:[],duration_ms:5000},{name:'oasis-volley',when:()=>trace.some(t=>t.view.projectiles.some(p=>p.owner===boss.id))});}
  finally{clearInterval(timer);if(pending)await pending;}
  if(sampleError)throw sampleError;
  await writeFile(join(out,'attacks.json'),JSON.stringify(trace));
  const errors=[],gaps=[];
  for(const t of trace){
    const groups=Map.groupBy(t.view.projectiles.filter(p=>p.owner===boss.id),p=>JSON.stringify(p.start));
    for(const shots of groups.values())if(shots.length===4){
      const aim=Math.atan2(t.view.position.y-shots[0].start.y,t.view.position.x-shots[0].start.x),angles=shots.map(p=>Math.atan2(Math.sin(p.angle-aim),Math.cos(p.angle-aim))).sort((a,b)=>a-b);
      errors.push(angles.reduce((a,b)=>a+b)/4);
      gaps.push(...angles.slice(1).map((a,i)=>a-angles[i]));
    }
  }
  assert(errors.length,'At least one complete native Giant volley required');
  // Recovered Oasis Giant Shoot omits angle, so the documented 9-degree default applies (monsters.xml:479; docs/reference-sources.md defaults).
  game.check('Giant volley has four bolts spaced nine degrees',gaps.every(g=>Math.abs(g-9*Math.PI/180)<.001));
  game.check('Giant aims at a stationary native player instead of a fixed angular offset',errors.every(e=>Math.abs(e)<.03));
  game.check('Giant wanders around its natural island',trace.some(t=>t.boss&&Math.hypot(t.boss.pos.x-boss.pos.x,t.boss.pos.y-boss.pos.y)>.1));
  await idle(50000);
  let state=await save('population');
  game.check('all four ordinary Oasis species participate',[0x679,0x67a,0x67b,0x67c].every(k=>children(state).some(e=>e.kind===k)));
  game.check('Giant reaches its twenty-child live cap',children(state).length===20);
  const population=children(state),attackers=new Set(trace.flatMap(t=>t.view.projectiles.map(p=>p.owner)));
  for(let n=0;n<5;n++){await idle(1000);state=await save('family-'+n);for(const p of state.view.projectiles)attackers.add(p.owner);}
  game.check('all four species fire native-visible projectiles',[0x679,0x67a,0x67b,0x67c].every(k=>population.some(e=>e.kind===k&&attackers.has(e.id))));
  game.check('all four species move from their actual birth cells during the encounter',[0x679,0x67a,0x67b,0x67c].every(k=>[...trace.flatMap(t=>t.children),...population,...children(state)].some(e=>e.kind===k&&Math.hypot(e.pos.x-e.home.x,e.pos.y-e.home.y)>.1)));
  async function fireUntilRetired(id){
    for(let n=0;n<40;n++){
      await protect();
      const {view,diagnostic}=await game.inspect(),enemy=diagnostic.entities.find(e=>e.id===id);
      if(!enemy)return;
      // The controller cannot aim at an already-lethal but not yet retired actor.
      if(enemy.hp<=0){await idle(250);continue;}
      const local=view.objects.find(e=>e.id===id&&e.hp>0);
      const dx=enemy.pos.x-view.position.x,dy=enemy.pos.y-view.position.y;
      const onGiant=id===boss.id;
      if(local&&Math.hypot(dx,dy)<(onGiant?2:5)){
        await game.act({keys:[],duration_ms:600,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
      }else{
        // Follower combat can leave the Archer off the entry causeway, behind projectile cover.
        const centerline=placement.root.y-view.position.y;
        const align=onGiant&&view.position.x>placement.root.x+3&&Math.abs(centerline)>.3;
        const x=align?-centerline:(dx-dy)/Math.SQRT2,y=align?centerline:(dx+dy)/Math.SQRT2;
        const keys=[...(Math.abs(x)>Math.abs(y)*.4142?[x>0?'d':'a']:[]),...(Math.abs(y)>Math.abs(x)*.4142?[y>0?'s':'w']:[])];
        await game.act({keys,duration_ms:align?100:200});
      }
    }
  }
  const heading=e=>Math.atan2(e.pos.y-state.view.position.y,e.pos.x-state.view.position.x),bossAngle=heading(state.diagnostic.entities.find(e=>e.id===boss.id));
  const victim=children(state).find(e=>state.view.objects.some(o=>o.id===e.id&&o.hp>0)&&Math.abs(Math.atan2(Math.sin(heading(e)-bossAngle),Math.cos(heading(e)-bossAngle)))>.4);assert(victim,'Visible follower outside the boss firing line required');
  await fireUntilRetired(victim.id);
  state=await save('follower-defeated');
  game.check('native bow fire defeats an owned follower',!state.diagnostic.entities.some(e=>e.id===victim.id));
  // Recovered MakeMinions prunes dead handles before checking maxChildren (behavior.cc:1076–1095).
  const originalIds=new Set(population.map(e=>e.id));await idle(10000);state=await save('replenishment');
  game.check('living Giant replaces a defeated follower within the live cap',children(state).length===20&&!children(state).some(e=>e.id===victim.id)&&children(state).some(e=>!originalIds.has(e.id)));
  const xp=state.view.character.experience;
  await fireUntilRetired(boss.id);
  await idle(500);state=await save('giant-defeated');
  game.check('native bow fire defeats the original Giant and earns XP',!state.diagnostic.entities.some(e=>e.id===boss.id)&&state.view.character.experience>xp);
  game.check('independent Oasis chest retains its precombat contents',state.diagnostic.entities.some(e=>e.id===chest.id&&JSON.stringify(e.items)===JSON.stringify(chest.items)));
  for(let n=0;n<60;n++){
    const v=await game.observe(),dx=chest.pos.x-v.position.x,dy=chest.pos.y-v.position.y;if(Math.hypot(dx,dy)<.4)break;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
    const distance=keys.length===2?Math.min(Math.abs(x),Math.abs(y))*Math.SQRT2:Math.max(Math.abs(x),Math.abs(y));
    await protect();await game.act({keys,duration_ms:Math.max(50,Math.min(250,Math.round(distance/(4+5.6*v.character.stats.speed/75)*1000)))});
  }
  const arrived=await game.observe();game.check('ordinary native walking reaches the Oasis chest',Math.hypot(arrived.position.x-chest.pos.x,arrived.position.y-chest.pos.y)<.4);
  await game.act({keys:[],duration_ms:700},{name:'oasis-chest',when:()=>true});
  const before=await game.observe(),item=chest.items[0];assert(item!==-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await save('picked');
  game.check('native drag commits one chest item',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===before.character.inventory_revision+1&&picked.diagnostic.entities.find(e=>e.id===chest.id)?.items[0]===-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});
  game.check('repeating an empty chest drag cannot duplicate loot',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  await game.act({keys:['Insert'],duration_ms:350});const returned=await game.observe();
  game.check('ordinary Nexus return retains the same living character and chest item',returned.world==='Nexus'&&returned.lifecycle.alive&&returned.session.character===initial.view.session.character&&returned.character.items[4]===item&&returned.character.inventory_revision===picked.view.character.inventory_revision);
}
