import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
export const realmSeed=0;
const spec={class:0x300,level:8,seed:814,world:1,equipment:[0xa19,0xb27,0xa0d,-1],backpack:Array(8).fill(-1),stats:[720,252,50,25,50,75,40,50]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
let coil,chest,blocked;
const clearLine=(a,b)=>Array.from({length:Math.ceil(Math.hypot(b.x-a.x,b.y-a.y)*10)+1},(_,i)=>{const t=i/Math.max(1,Math.ceil(Math.hypot(b.x-a.x,b.y-a.y)*10));return !blocked.has([Math.floor(a.x+(b.x-a.x)*t),Math.floor(a.y+(b.y-a.y)*t)].join(','));}).every(Boolean);
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'coil-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json();blocked=new Set(map.blocked.map(p=>p.join(',')));
  const coils=map.entities.filter(e=>e.kind===0xdc2),chestAt=e=>map.entities.find(c=>c.kind===0x501&&c.pos.x===e.pos.x&&c.pos.y===e.pos.y);
  coil=coils.find(e=>chestAt(e)&&[-3,-2,-1,0].every(dx=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)].join(','))));assert(coil,'No natural chest temple Coil with a clear approach among '+coils.length);
  chest=chestAt(coil);
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-temples.json'),JSON.stringify({world,coils,chest}));
  return {...spec,position:{x:coil.pos.x-3,y:coil.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  const toward=(from,to)=>{const dx=to.x-from.x,dy=to.y-from.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;return [Math.abs(x)>.15&&Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>.15&&Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean);};
  async function defeat(id){
    for(let n=0;n<45;n++){
      const s=await game.inspect(),e=entity(s,id);if(!e)break;
      const keys=Math.hypot(e.pos.x-s.view.position.x,e.pos.y-s.view.position.y)>3?toward(s.view.position,e.pos):[];
      await game.act({keys,duration_ms:250,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    }
    game.check(id+': native dagger defeats target',!entity(await game.inspect(),id));
  }
  async function walk(point){
    for(let n=0;n<60;n++){
      const v=await game.observe(),distance=Math.hypot(point.x-v.position.x,point.y-v.position.y);if(distance<.4)return;
      await game.act({keys:toward(v.position,point),duration_ms:Math.max(50,Math.min(250,Math.round(distance/(4+5.6*v.character.stats.speed/75)*1000)))});
    }
    throw Error('Native approach did not reach '+JSON.stringify(point));
  }
  const initial=await save('arrival');
  game.check('natural temple has selected 900-HP Coil and stocked chest',entity(initial,coil.id)?.hp===900&&entity(initial,chest.id)?.items.some(i=>i!==-1));
  const samples=[],shots=new Map();let pending=null,error;
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)if(!shots.has(p.owner+':'+p.id))shots.set(p.owner+':'+p.id,p);};
  async function sample(input,capture){
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},65);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;
  }
  await sample({keys:[],duration_ms:6000},{name:'coil-arrival',when:v=>v.game_ms>3500});
  await save('exposed');
  game.check('native enemy attacks damage the Rogue',samples.some(s=>s.view.character.hp<initial.view.character.hp));
  const path=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(path,JSON.stringify({id:'coil-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);
  let livingEgg;
  for(let n=0;n<24;n++){
    const s=await game.inspect();
    livingEgg=s.diagnostic.entities.find(e=>e.kind===0xdc1&&e.parent===coil.id&&e.activated!==null&&s.diagnostic.time_ms-e.activated<3000&&Math.hypot(e.pos.x-s.view.position.x,e.pos.y-s.view.position.y)<4.5&&clearLine(s.view.position,e.pos));
    if(livingEgg)break;await sample({keys:[],duration_ms:500});
  }
  assert(livingEgg,'No young natural egg with a clear dagger shot');
  const beforeEgg=await save('before-egg');
  await game.act({keys:[],duration_ms:50,target:{entity:livingEgg.id,camera_angle:-Math.PI/4,fire:true}});
  await game.act({keys:[],duration_ms:400});
  const afterEgg=await save('egg-destroyed');
  // A Coil can lay a fresh sibling between the inspect and the native hit. Match its durable terminal receipt to the observed Coil, not to one snapshot.
  const {DatabaseSync}=await import('node:sqlite');
  const db=new DatabaseSync(join(root,'state/game.db'),{readOnly:true});
  const eggKills=db.prepare("SELECT json_extract(payload,'$.payload.data.defeat.actor') actor FROM world_events WHERE kind='terminal' AND status=2 AND json_extract(payload,'$.payload.data.defeat.kind')=3521 AND json_extract(payload,'$.payload.data.actions.publication.parent')=? AND json_extract(payload,'$.payload.data.actions.publication.reason')='Defeated'").all(coil.id);
  db.close();
  const shotEgg=eggKills[0]?.actor;
  game.check('the native shot kills one existing Coil egg without XP or a hatch from that egg',eggKills.length===1&&!entity(afterEgg,shotEgg)&&
    afterEgg.view.character.experience===beforeEgg.view.character.experience&&afterEgg.diagnostic.character.lifetime.kills===beforeEgg.diagnostic.character.lifetime.kills+1&&
    !afterEgg.diagnostic.entities.some(e=>e.kind===0xdc0&&e.parent===shotEgg));
  await sample({keys:[],duration_ms:6000},{name:'coil-eggs',when:v=>v.game_ms>9000});
  await sample({keys:[],duration_ms:6000});
  const active=await save('active'),xp=active.view.character.experience;
  game.check('natural Coil activates and wanders',samples.some(s=>{const e=entity(s,coil.id);return e?.activated!==null&&Math.hypot(e.pos.x-coil.pos.x,e.pos.y-coil.pos.y)>.1;}));
  const aimed=[...shots.values()].filter(p=>p.owner===coil.id&&p.speed===65),rings=[...shots.values()].filter(p=>p.owner===coil.id&&p.speed===45);
  game.check('native client receives selected long-lived Snake Shot',aimed.length>0&&aimed.every(p=>p.lifetime_ms===16000));
  // XML rings are targeted (actors.xml:10278), so each volley is ten 36-degree rays aimed at the player, not a fixed rosette.
  const volleys=Map.groupBy(rings,p=>p.start.x+':'+p.start.y);
  game.check('native client receives ten-way 36-degree Ghost Shot rings',[...volleys.values()].some(g=>g.length===10&&new Set(g.map(p=>Math.round(((p.angle*180/Math.PI)%36+36)%36)%36)).size===1)&&rings.every(p=>p.lifetime_ms===1000));
  // XML eggs hatch after 5.4s into one or two Great Temple Snakes (actors.xml:10258-10266).
  const firstSeen=kind=>{const m=new Map();for(const s of samples)for(const e of s.diagnostic.entities)if(e.kind===kind&&e.activated!==null&&!m.has(e.id))m.set(e.id,e);return [...m.values()];};
  const snakes=firstSeen(0xdc0),hatched=firstSeen(0xdc1).filter(e=>e.id!==shotEgg).map(egg=>({id:egg.id,activated:egg.activated,children:snakes.filter(c=>c.parent===egg.id)})).filter(h=>h.children.length);
  game.check('natural eggs hatch after 5.4 seconds into one or two Snakes without XP',hatched.length>0&&hatched.every(h=>h.children.length<=2&&h.children.every(c=>c.activated-h.activated>=5400&&c.activated-h.activated<=5800))&&xp===afterEgg.view.character.experience);
  game.check('Temple Snake attacks reach the native client',[...shots.values()].some(p=>p.owner!==coil.id&&samples.some(s=>entity(s,p.owner)?.kind===0xdc0)&&p.speed===55&&p.lifetime_ms===800));
  const cloakStart=(await game.inspect()).diagnostic.time_ms;
  // A single tap cloaks; a 6s held key outlives the native hold under load.
  await sample({keys:[' '],duration_ms:250});await sample({keys:[],duration_ms:5750});
  const cloaked=samples.filter(s=>s.diagnostic.time_ms>cloakStart+1000&&(s.view.character.conditions&(1<<12)));
  game.check('ordinary cloak is observed',cloaked.length>0);
  // Every XML Coil Shoot is targeted (actors.xml:10276-10278), so no new ring is aimed at the only, cloaked player.
  const before=new Set(samples.filter(s=>s.diagnostic.time_ms<=cloakStart).flatMap(s=>s.view.projectiles.map(p=>p.owner+':'+p.id)));
  game.check('targeted Ghost Shot rings stop while the only nearby player is cloaked',!cloaked.some(s=>s.view.projectiles.some(p=>p.owner===coil.id&&p.speed===45&&!before.has(p.owner+':'+p.id)&&s.diagnostic.time_ms>cloakStart+1500)));
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify({samples,shots:[...shots.values()],hatched}));

  await defeat(coil.id);await game.act({keys:[],duration_ms:500});
  const defeated=await save('coil-defeated');
  game.check('Coil defeat awards XP',defeated.view.character.experience>afterEgg.view.character.experience);
  const snake=defeated.diagnostic.entities.find(e=>e.kind===0xdc0);assert(snake,'No surviving Temple Snake');await defeat(snake.id);await game.act({keys:[],duration_ms:500});
  const credited=await save('snake-defeated');
  game.check('Temple Snake defeat awards XP',credited.view.character.experience>defeated.view.character.experience);
  await game.act({keys:[],duration_ms:6000},{name:'coil-defeated',when:()=>true});
  const final=await save('final');
  game.check('Coil defeat credit is not repeated',final.view.character.experience===credited.view.character.experience);
  game.check('same supplied living Rogue completes native encounter',final.view.lifecycle.alive&&final.view.session.character===initial.view.session.character);
  await walk(chest.pos);
  const inventory=await game.observe(),item=entity(final,chest.id).items[0];
  game.check('ordinary movement reaches the natural chest',Math.hypot(inventory.position.x-chest.pos.x,inventory.position.y-chest.pos.y)<.4&&item!==-1);
  await game.act({keys:[],duration_ms:700},{name:'coil-chest',when:()=>true});
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await save('chest-picked');
  game.check('native drag acquires natural chest loot once',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===inventory.character.inventory_revision+1&&entity(picked,chest.id).items[0]===-1);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});
  game.check('empty chest slot cannot duplicate loot',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  const portals=(await game.inspect()).diagnostic.entities.filter(e=>e.kind===0x733);
  await writeFile(join(process.env.QA_RESULTS_DIR,'outcome.json'),JSON.stringify({coil:coil.id,destroyedEgg:livingEgg.id,portals,portalEntryObserved:false}));
  if(portals.length){
    assert.equal(portals.length,1);await walk(portals[0].pos);
    await game.act({keys:[],duration_ms:700},{name:'coil-portal',when:()=>true});
    await game.act({keys:[],mouse:{x:700,y:580},duration_ms:1500});
    const entered=await save('jungle-entered');
    game.check('native interaction enters the earned Forbidden Jungle portal',entered.view.map.name==='Forbidden Jungle'&&entered.view.lifecycle.alive&&entered.view.session.character===initial.view.session.character);
    await game.act({keys:[],duration_ms:2500});
    await game.act({keys:['d'],duration_ms:300});
    const moving=await save('jungle-moving');
    game.check('Jungle admission finishes with ordinary native movement',moving.view.map.name==='Forbidden Jungle'&&Math.hypot(moving.view.position.x-entered.view.position.x,moving.view.position.y-entered.view.position.y)>.1);
    await game.act({keys:[],duration_ms:700},{name:'jungle-entered',when:()=>true});
    await writeFile(join(process.env.QA_RESULTS_DIR,'outcome.json'),JSON.stringify({coil:coil.id,destroyedEgg:livingEgg.id,portals,portalEntryObserved:true}));
  }
}
