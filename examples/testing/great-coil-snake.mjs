import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x300,level:8,seed:814,world:1,equipment:[0xa14,0xb27,0xa0d,-1],backpack:Array(8).fill(-1),stats:[720,252,50,25,50,75,40,50]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
let coil,chest;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'coil-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const coils=map.entities.filter(e=>e.kind===0xdc2);assert.equal(coils.length,2);
  coil=coils.find(e=>[-3,-2,-1,0].every(dx=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)].join(','))));assert(coil,'No natural Coil with a clear approach');
  chest=map.entities.find(e=>e.kind===0x501&&e.pos.x===coil.pos.x&&e.pos.y===coil.pos.y);assert(chest,'Natural temple lacks its chest');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-temples.json'),JSON.stringify({world,coils,chest}));
  return {...spec,position:{x:coil.pos.x-3,y:coil.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  const initial=await save('arrival');
  game.check('natural temple has selected 900-HP Coil and stocked chest',entity(initial,coil.id)?.hp===900&&entity(initial,chest.id)?.items.some(i=>i!==-1));
  await game.act({keys:[],duration_ms:2400},{name:'coil-arrival',when:()=>true});
  const exposed=await save('exposed');
  game.check('native enemy attacks damage the Rogue',exposed.view.character.hp<initial.view.character.hp);
  const path=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(path,JSON.stringify({id:'coil-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);
  const samples=[],shots=new Map();let pending=null,error;
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)if(!shots.has(p.owner+':'+p.id))shots.set(p.owner+':'+p.id,p);};
  async function sample(input,capture){
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},65);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;
  }
  await sample({keys:[],duration_ms:6000},{name:'coil-eggs',when:v=>v.game_ms>6500});
  await sample({keys:[],duration_ms:6000});
  const active=await save('active'),xp=active.view.character.experience;
  game.check('natural Coil activates and wanders',samples.some(s=>{const e=entity(s,coil.id);return e?.activated!==null&&Math.hypot(e.pos.x-coil.pos.x,e.pos.y-coil.pos.y)>.1;}));
  const aimed=[...shots.values()].filter(p=>p.owner===coil.id&&p.speed===65),rings=[...shots.values()].filter(p=>p.owner===coil.id&&p.speed===45);
  game.check('native client receives selected long-lived Snake Shot',aimed.length>0&&aimed.every(p=>p.lifetime_ms===16000));
  game.check('native client receives ten-way 36-degree Ghost Shot rings',rings.length>=10&&new Set(rings.map(p=>Math.round((p.angle*180/Math.PI+360)%360))).size===10&&rings.every(p=>p.lifetime_ms===1000));
  const eggSeen=new Map(),hatched=[];
  for(const s of samples){
    for(const e of s.diagnostic.entities.filter(e=>e.kind===0xdc1))eggSeen.set(e.id,e);
    for(const [id,e] of eggSeen)if(!entity(s,id)&&s.diagnostic.entities.some(c=>c.kind===0xdc0&&Math.hypot(c.pos.x-e.pos.x,c.pos.y-e.pos.y)<1)&&!hatched.includes(id))hatched.push(id);
  }
  // This period witness supports surviving-egg hatching, not later TransformOnDeath: https://web.archive.org/web/20120419234213id_/http://forums.wildshadow.com/wiki/realm-mad-god/great-coil-snake
  game.check('natural tossed eggs hatch into Temple Snakes without XP',hatched.length>0&&xp===exposed.view.character.experience);
  game.check('Temple Snake attacks reach the native client',[...shots.values()].some(p=>p.owner!==coil.id&&samples.some(s=>entity(s,p.owner)?.kind===0xdc0)&&p.speed===55&&p.lifetime_ms===800));
  await sample({keys:[' '],duration_ms:6000});
  const cloaked=samples.filter(s=>s.view.character.conditions&(1<<12));
  game.check('ordinary cloak is observed',cloaked.length>0);
  game.check('fixed Ghost Shot rings continue while the only nearby player is cloaked',cloaked.some(s=>s.view.projectiles.some(p=>p.owner===coil.id&&p.speed===45)));
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify({samples,shots:[...shots.values()],hatched}));
  async function defeat(id){for(let n=0;n<45&&entity(await game.inspect(),id);n++)await game.act({keys:[],duration_ms:250,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});game.check(id+': native dagger defeats target',!entity(await game.inspect(),id));}
  const egg=active.diagnostic.entities.find(e=>e.kind===0xdc1),livingEgg=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0xdc1);
  assert(livingEgg,'No surviving egg to destroy');
  const beforeEgg=await game.inspect();await defeat(livingEgg.id);
  const afterEgg=await save('egg-destroyed');
  game.check('destroying an egg gives no XP',afterEgg.view.character.experience===beforeEgg.view.character.experience);
  await defeat(coil.id);
  const defeated=await save('coil-defeated');
  game.check('Coil defeat awards XP',defeated.view.character.experience>afterEgg.view.character.experience);
  const snake=defeated.diagnostic.entities.find(e=>e.kind===0xdc0);assert(snake,'No surviving Temple Snake');await defeat(snake.id);
  const credited=await save('snake-defeated');
  game.check('Temple Snake defeat awards XP',credited.view.character.experience>defeated.view.character.experience);
  await game.act({keys:[],duration_ms:6000},{name:'coil-defeated',when:()=>true});
  const final=await save('final');
  game.check('Coil defeat credit is not repeated',final.view.character.experience===credited.view.character.experience);
  game.check('same supplied living Rogue completes native encounter',final.view.lifecycle.alive&&final.view.session.character===initial.view.session.character);
  await writeFile(join(process.env.QA_RESULTS_DIR,'outcome.json'),JSON.stringify({coil:coil.id,destroyedEgg:livingEgg.id,earlierEgg:egg?.id,portals:final.diagnostic.entities.filter(e=>e.kind===0x733)}));
}
