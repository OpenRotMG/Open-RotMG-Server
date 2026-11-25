import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
let camp,leader,mapFor,blocked,cover,width,height;
export const realmSeed=1296;
export async function prepare(api){
  const spec={class:0x30e,level:20,seed:840,world:1,equipment:[0xb08,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,30,10,0,0]};
  const root=process.env.QA_RESULTS_DIR,report=JSON.parse(await readFile(join(root,'native/result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'native/state/control.token'),'utf8')).trim()};
  const world=await api({op:'world_info',world:spec.world}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'warg-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const full=async player=>{const response=await fetch(report.origin+'/dev/map?player='+player,{headers});assert(response.ok);return response.json();};
  // Keep only entities: retaining whole Realm maps exhausts the runner heap.
  mapFor=async player=>({entities:(await full(player)).entities});
  const map=await full(state.diagnostic.player),candidates=[],placements=[];
  ({width,height}=map);blocked=new Set(map.blocked.map(p=>p.join(',')));
  const [enemyTypes,coverTypes]=JSON.parse(execFileSync('python3',['-c',"import json,xml.etree.ElementTree as E; o=list(E.parse('data/objects.xml').getroot()); ids=lambda pred:[int(e.attrib['type'],0) for e in o if pred(e)]; print(json.dumps([ids(lambda e:e.find('Enemy') is not None),ids(lambda e:e.find('Static') is not None and e.find('Enemy') is None and (e.find('OccupySquare') is not None or e.find('EnemyOccupySquare') is not None))]))"],{encoding:'utf8'}));
  const enemies=new Set(enemyTypes),coverKinds=new Set(coverTypes);
  cover=new Set(map.entities.filter(e=>coverKinds.has(e.kind)).map(e=>[Math.floor(e.pos.x),Math.floor(e.pos.y)].join(',')));
  for(const e of map.entities.filter(e=>e.kind===0x623)){
    const distance=Math.min(...map.entities.filter(p=>p.camp!==e.camp&&enemies.has(p.kind)).map(p=>Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)));
    const clear=[-1,0,1].every(dx=>[-1,0,1].every(dy=>!blocked.has([Math.floor(e.pos.x)-3+dx,Math.floor(e.pos.y)+dy].join(','))));
    placements.push({id:e.id,distance,clear});if(distance>=12&&clear)candidates.push({entity:e,distance});
  }
  await writeFile(join(root,'camp-selection.json'),JSON.stringify({world:spec.world,layout:world.realm_layout,placements}));
  candidates.sort((a,b)=>b.distance-a.distance);assert(candidates.length,'No natural Warg camp has a clear approach and twelve-tile separation from other actors');
  leader=candidates[0].entity;camp=leader.camp;
  await writeFile(join(root,'natural-family.json'),JSON.stringify({world:spec.world,layout:world.realm_layout,clearance:candidates[0].distance,members:map.entities.filter(e=>e.camp===camp)}));
  return {...spec,position:{x:leader.pos.x-3,y:leader.pos.y}};
}
// Orphans Wander (actors.xml:741–756). In the linked Realm, both canonical retries
// drove into the same solid tile while chasing 262703. Projectile cover differs from player
// collision (src/world.rs:4693 and 6704–6712); use each mask for its own check.
function clearLine(a,b){
  const samples=Math.max(1,Math.ceil(8*Math.hypot(b.x-a.x,b.y-a.y)));
  for(let i=0;i<=samples;i++)if(cover.has([Math.floor(a.x+(b.x-a.x)*i/samples),Math.floor(a.y+(b.y-a.y)*i/samples)].join(',')))return false;
  return true;
}
function firingApproach(pos,targets,range){
  const origin={x:Math.floor(pos.x),y:Math.floor(pos.y)},queue=[origin],seen=new Set([[origin.x,origin.y].join(',')]);
  for(let i=0;i<queue.length;i++){
    const at=queue[i],center={x:at.x+.5,y:at.y+.5};
    const from=at.first?center:pos;
    const target=targets.find(e=>Math.hypot(from.x-e.pos.x,from.y-e.pos.y)<=range&&clearLine(from,e.pos));
    if(target)return {target,first:at.first};
    for(const [dx,dy] of [[1,0],[0,1],[-1,0],[0,-1]]){
      const x=at.x+dx,y=at.y+dy,key=[x,y].join(',');
      if(x<0||y<0||x>=width||y>=height||seen.has(key)||blocked.has(key)||Math.abs(x-origin.x)+Math.abs(y-origin.y)>64)continue;
      seen.add(key);queue.push({x,y,first:at.first??{x:x+.5,y:y+.5}});
    }
  }
}
export async function run(game){
  const root=process.env.QA_RESULTS_DIR;
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  const save=async name=>{const s=await game.inspect();await writeFile(join(root,name+'.json'),JSON.stringify(s));return s;};
  let protectedUntil=0,serial=0;
  async function act(input,capture){
    while(protectedUntil&&(await game.observe()).game_ms<protectedUntil&&(await game.observe()).game_ms+input.duration_ms+1500>=protectedUntil)await game.act({keys:[],duration_ms:100});
    const now=(await game.observe()).game_ms;
    if(now>=protectedUntil){
      const path=join(root,'protection.json');
      await writeFile(path,JSON.stringify({id:'warg-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native/native.json'),path]);protectedUntil=now+60000;
    }
    return game.act(input,capture);
  }
  await act({keys:[],duration_ms:1000},{name:'wargs-arrival',when:()=>true});
  const player=(await game.inspect()).diagnostic.player;
  const camera=-Math.PI/4;
  const fired=new Set();
  async function shoot(ids,lifetime=0){
    let id=ids.length===1?ids[0]:undefined,locked=!lifetime;
    const routeTrace=[];
    for(let n=0;n<160;n++){
      const s=await game.inspect(),pos=s.view.position;
      const options=locked?[id]:ids;
      const missing=options.filter(candidate=>!entity(s,candidate));
      const offscreen=missing.length?(await mapFor(player)).entities:[];
      const targets=options.map(candidate=>entity(s,candidate)??offscreen.find(e=>e.id===candidate)).filter(Boolean);
      if(locked&&!targets.length)return id;
      const route=firingApproach(pos,targets,lifetime===600?3.2:4.2);
      if(route){
        id=route.target.id;
        if(s.view.projectiles.some(p=>p.owner===id&&p.speed===60&&p.lifetime_ms===lifetime)){
          fired.add(id+':'+lifetime);locked=true;
        }
      }
      routeTrace.push({pos,targets:targets.map(e=>[e.id,e.pos]),id,locked,next:route?.first});
      // A wandering target may briefly enter player-blocked scenery. Re-observe it within the same input budget.
      if(!route||!route.first&&!s.view.objects.some(o=>o.id===id)){await act({keys:[],duration_ms:100});continue;}
      const next=route.first??pos,approach=!!route.first;
      const dx=next.x-pos.x,dy=next.y-pos.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      const keys=approach?[...(Math.abs(x)>Math.abs(y)*.4142?[x>0?'d':'a']:[]),...(Math.abs(y)>Math.abs(x)*.4142?[y>0?'s':'w']:[])]:[];
      // Orphan Wargs only Shoot a player inside their 3.4/4.5-tile range (actors.xml:744,753), so hold fire there first.
      if(!approach&&!locked){await act({keys:[],duration_ms:100});continue;}
      await act({keys,duration_ms:100,...(approach?{}:{target:{entity:id,camera_angle:camera,fire:true}})});
    }
    await writeFile(join(root,'warg-route-failure.json'),JSON.stringify(routeTrace));
    assert.fail('Native approach / shots failed for Wargs '+ids.join(',')+'; selected '+id+'; projectile observed '+locked);
  }
  // The Werewolf's XML MakeMinions Wargs (actors.xml:762) are owned by parent; its leader-only camp id is reallocated once it dies.
  const arrival=await save('arrival'),family=m=>m.entities.filter(e=>e.id===leader.id||e.parent===leader.id);
  await shoot([leader.id]);await act({keys:[],duration_ms:500},{name:'wargs-orphaned',when:()=>true});const orphaned=await save('orphaned'),orphans=family(await mapFor(player));
  game.check('native owner defeat leaves both Warg types alive',!entity(orphaned,leader.id)&&[0x620,0x622].every(k=>orphans.some(e=>e.kind===k)));
  // XML orphans Wander (actors.xml:741-756) and may leave the native neighborhood, so census them in the whole world.
  const samples=[];
  for(let n=0;n<26;n++){await act({keys:[],duration_ms:500});samples.push({experience:(await game.observe()).character.experience,map:{entities:family(await mapFor(player))}});}
  await writeFile(join(root,'orphan-samples.json'),JSON.stringify(samples));
  for(const kind of [0x620,0x622]){
    const initial=orphans.filter(e=>e.kind===kind),at=(m,id)=>m.entities.find(e=>e.id===id);
    game.check(kind+': same orphans persist and move after owner death',initial.length>0&&initial.every(e=>samples.every(s=>at(s.map,e.id)))&&initial.some(e=>samples.some(s=>Math.hypot(at(s.map,e.id).pos.x-e.pos.x,at(s.map,e.id).pos.y-e.pos.y)>.1)));
  }
  game.check('dead owner creates no replacement Wargs or repeat XP',samples.every(s=>family(s.map).every(e=>orphans.some(p=>p.id===e.id))&&s.experience===orphaned.view.character.experience));
  const victims=[];
  for(const [kind,xp,lifetime] of [[0x620,3,600],[0x622,4,2000]]){
    const before=await game.inspect(),candidates=family(await mapFor(player)).filter(e=>e.kind===kind);
    assert(candidates.length,'Missing natural orphan kind '+kind);
    const id=await shoot(candidates.map(e=>e.id),lifetime);victims.push(id);
    await act({keys:[],duration_ms:500});const dead=await save('orphan-defeated-'+kind);
    game.check(kind+': a nearby orphan fires its selected projectile',fired.has(id+':'+lifetime));
    game.check(kind+': native orphan defeat earns XP',!entity(dead,id)&&dead.view.character.experience>=before.view.character.experience+xp);
  }
  const dead=await save('orphans-defeated'),survivors=family(await mapFor(player));
  for(let n=0;n<24;n++)await act({keys:[],duration_ms:500});const end=await save('finished'),last=family(await mapFor(player));
  game.check('defeated orphans stay absent without repeated XP',victims.every(id=>!last.some(e=>e.id===id))&&last.every(e=>survivors.some(p=>p.id===e.id))&&end.view.character.experience===dead.view.character.experience);
  game.check('same prepared character survives',end.view.lifecycle.alive&&end.view.session.character===arrival.view.session.character);
}
