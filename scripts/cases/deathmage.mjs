import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {realmSiteTerrain} from './realm-site-terrain.mjs';
const out=process.env.QA_RESULTS_DIR;
const spec={class:0x300,level:20,seed:795,world:1,equipment:[0xaff,0xb27,0xaf9,-1],backpack:Array(8).fill(-1),stats:[720,252,50,0,75,75,0,0]};
const adjacentStocked=(chest,site)=>chest.kind===0x501&&Math.hypot(chest.pos.x-site.x,chest.pos.y-site.y)<3&&chest.items.some(i=>i!==-1);
let root;
export async function prepare(api){
  const world=await api({op:'world_info',world:1});
  const events=(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  const end=events.findLastIndex(f=>f.message==='Realm simulation populated'&&f.layout===world.realm_layout);assert(end>=0);
  const start=events.slice(0,end).findLastIndex(f=>f.message==='Realm simulation populated');
  const roots=events.slice(start+1,end).filter(f=>f.message==='linked Realm site placed'&&f.kind===0x925);
  // game.xml NamedSite keeps the 79a4c92b count, with the recovered Graveyard's MidPlains.
  const terrain=await realmSiteTerrain(world.realm_layout);
  assert(roots.length>=5&&roots.length<=9&&roots.every(r=>terrain(r.x,r.y)===6),'Expected 5–9 Deathmage sites in MidPlains');
  // The authored chest rolls can be empty (game.xml LandmarkChest Deathmage); scout a naturally stocked site for the pickup contract.
  const [x,y]=world.spawn_points[0],scout=await api({op:'spawn',spec:{...spec,guid:'deathmage-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),report=JSON.parse(await readFile(join(out,'native/result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(out,'native/state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json();
  root=roots.find(site=>map.entities.some(e=>adjacentStocked(e,site)));
  assert(root,'No naturally stocked Deathmage chest in the prepared Realm');
  await writeFile(join(out,'placement.json'),JSON.stringify({world,roots,selected:root}));
  return {...spec,position:{x:root.x,y:root.y}};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const records=[];let protectedUntil=0;
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));protectedUntil=now+60000;
  }
  async function save(label){const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounter.json'),JSON.stringify(records));return state;}
  const entity=(state,id)=>state.diagnostic.entities.find(e=>e.id===id);
  // XML MakeMinions: Skeleton/Swordsman/Veteran/Mage capped 4/2/1/1, owned by parent (actors.xml Deathmage; testingObjects.xml:1944).
  const ROLES=[[0x65a,4],[0x65b,2],[0x65c,1],[0x65d,1]];
  const family=state=>state.diagnostic.entities.filter(e=>e.parent===root.entity&&ROLES.some(([kind])=>kind===e.kind));
  async function approach(point,range=.4){
    for(let n=0;n<60;n++){
      await protect();const view=await game.observe(),dx=point.x-view.position.x,dy=point.y-view.position.y;if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.max(50,Math.min(200,Math.round(Math.hypot(dx,dy)/(4+5.6*view.character.stats.speed/75)*1000)))});
    }
    throw Error('Native walk did not reach observed target');
  }
  async function defeat(id){
    for(let n=0;n<80;n++){
      await protect();const state=await game.inspect(),target=entity(state,id);if(!target)return;
      if(Math.hypot(target.pos.x-state.view.position.x,target.pos.y-state.view.position.y)>4)await approach(target.pos,2);
      await game.act({keys:[],duration_ms:250,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    }
    throw Error('Native dagger shots did not defeat '+id);
  }
  const untouched=await game.observe();
  for(let n=0;n<8&&(await game.observe()).character.hp===untouched.character.hp;n++)await game.act({keys:[],duration_ms:250});
  const wounded=await game.observe();
  game.check('native encounter projectiles reduce living player HP',wounded.lifecycle.alive&&wounded.character.hp<untouched.character.hp);
  await protect();await game.act({keys:[],duration_ms:500},{name:'graveyard-damage',when:view=>view.character.hp<untouched.character.hp});
  let state=await save('activated');
  game.check('naturally placed Deathmage has selected 900 HP and self ownership',entity(state,root.entity)?.hp===900&&entity(state,root.entity).quest===root.entity);
  const chest=state.diagnostic.entities.find(e=>adjacentStocked(e,root));assert(chest,'Naturally rolled adjacent chest must contain an item');
  game.check('all four XML skeleton roles admitted within bounds',ROLES.every(([kind,max])=>{const count=family(state).filter(e=>e.kind===kind).length;return count>0&&count<=max;}));
  const initial=family(state);await game.act({keys:[],duration_ms:5000},{name:'graveyard-combat',when:()=>true});state=await save('attacks');
  game.check('Deathmage and every skeleton role emit native projectiles',[root.entity,...initial.map(e=>e.id)].every(id=>entity(state,id)?.bullet>0));
  await defeat(root.entity);await game.act({keys:[],duration_ms:500});state=await save('deathmage-defeated');
  game.check('ordinary dagger fire defeats Deathmage without a replacement boss',!entity(state,root.entity)&&family(state).length>0&&state.diagnostic.entities.filter(e=>e.quest===root.entity||e.parent===root.entity).every(e=>ROLES.some(([kind])=>kind===e.kind)));
  game.check('owned survivors keep running their own XML program after the issuer dies',family(state).every(e=>state.diagnostic.xml[e.id]?.active_ms>0));
  const counts=family(state).map(e=>e.id);await game.act({keys:[],duration_ms:10000});await game.act({keys:[],duration_ms:1000});state=await save('no-resummon');
  game.check('dead Deathmage cannot replenish after ten seconds',family(state).every(e=>counts.includes(e.id)));
  // Single-cap Veteran/Mage often fall to fire aimed at the Deathmage; defeat every surviving role.
  const survivors=family(state);for(const kind of new Set(survivors.map(e=>e.kind))){const target=survivors.find(e=>e.kind===kind);await defeat(target.id);game.check('native defeat of skeleton role '+kind,!entity(await game.inspect(),target.id));}
  await save('skeleton-rewards');
  await approach(chest.pos);await game.act({keys:[],duration_ms:500},{name:'chest-open',when:()=>true});
  const before=await game.observe(),item=chest.items[0];assert(item!==-1);
  // The period chest tier list establishes rewards, not rates: https://web.archive.org/web/20120610073636id_/http://forums.wildshadow.com:80/wiki/realm-mad-god/deathmage
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await save('chest-picked');
  game.check('native chest drag commits one item',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===before.character.inventory_revision+1&&entity(picked,chest.id)?.items[0]===-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});
  game.check('repeating empty chest slot does not duplicate loot',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  const xp=(await game.observe()).character.experience;await game.act({keys:[],duration_ms:2000});
  game.check('defeat XP remains once only',(await game.observe()).character.experience===xp);
  await game.act({keys:['Insert'],duration_ms:250});const retained=await game.observe();
  game.check('ordinary Nexus reconnect persists acquired chest item',retained.world==='Nexus'&&retained.lifecycle.alive&&retained.session.character===picked.view.session.character&&retained.character.items[4]===item&&retained.character.inventory_revision===picked.view.character.inventory_revision);
}
