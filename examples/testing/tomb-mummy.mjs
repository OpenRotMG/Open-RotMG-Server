import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

// Initial positions and selected maxed Knight gear are supplied, not an earned Tomb route.
export const fixture={class:0x31e,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position:'spawn',equipment:[0xb0b,0xb22,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
export async function run(observer){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await observer.inspect(),world=initial.view.session.world;
  const map=async()=>{const r=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});assert(r.ok,'Read-only Tomb map failed');return r.json();};
  const records=[],save=async(label,game)=>{const s={label,state:await game.inspect(),map:await map()};records.push(s);await writeFile(join(out,'mummy-lifecycle.json'),JSON.stringify(records));assert(s.state.view.lifecycle.alive,'Prepared Knight must remain alive');assert.equal(s.state.diagnostic.character.tombs,0,'Ordinary Mummy cannot grant a Tomb clear');return s;};
  const starting=await map(),mummies=starting.entities.filter(e=>e.kind===0xd1a&&e.hp===3000);
  assert(mummies.length,'Generated seed has no full-health Bloated Mummy');
  const blocked=new Set(starting.blocked.map(p=>p.join(',')));
  function point(mummy,radius,info){
    const options=[[1,0],[-1,0],[0,1],[0,-1]].map(([x,y])=>({x:mummy.pos.x+x*radius,y:mummy.pos.y+y*radius}));
    const p=options.find(p=>p.x>=1&&p.y>=1&&p.x<starting.width-1&&p.y<starting.height-1&&!blocked.has(`${Math.floor(p.x)},${Math.floor(p.y)}`)&&info.entities.every(e=>e.id===mummy.id||e.kind===0xd1a||(e.hp<=0&&!(e.kind>=0xd31&&e.kind<=0xd36)&&e.kind!==0xd38)||e.kind===0xd1b||distance(e.pos,p)>6));
    assert(p,'No walkable prepared observation position away from unrelated hostile roots');return p;
  }
  const mummy=mummies[0],{worldSeed,...gear}=fixture;
  const watching=await observer.join({...gear,world,position:point(mummy,4,starting)}),watchStart=await save('full-health-mummy',watching);
  assert(watchStart.map.entities.some(e=>e.id===mummy.id&&e.hp===3000));
  await watching.act({keys:[],duration_ms:750},{name:'tomb-mummy-normal-stars',when:v=>v.world==='Tomb of the Ancients'&&v.lifecycle.alive&&v.game_ms>=watchStart.state.view.time_ms+350});
  const normal=await save('ordinary-stars-and-first-scarabs',watching),live=normal.map.entities.find(e=>e.id===mummy.id);
  observer.check('unattacked full-health Mummy remains alive outside proximity range',live?.hp===3000&&normal.state.diagnostic.character.shots===0);
  observer.check('Mummy naturally emits selected Green Stars and creates five owned Scarabs',normal.state.view.projectiles.some(p=>p.owner===mummy.id)&&normal.map.entities.filter(e=>e.kind===0xd1b&&e.tomb?.producer===mummy.id).length===5);
  await watching.act({keys:['Insert'],duration_ms:300});
  assert.equal((await watching.observe()).world,'Nexus');
  const before=await map(),current=before.entities.find(e=>e.id===mummy.id);assert(current?.hp===3000);
  const originalScarabs=new Set(before.entities.filter(e=>e.kind===0xd1b&&e.tomb?.producer===mummy.id).map(e=>e.id));
  const approaching=await observer.join({...gear,world,position:point(current,1.5,before)}),near=await save('prepared-proximity-before-unpause',approaching);
  observer.check('proximity starts with the same untouched Mummy and no native weapon damage',near.map.entities.some(e=>e.id===mummy.id&&e.hp===3000)&&near.state.diagnostic.character.shots===0);
  await approaching.act({keys:[],duration_ms:700},{name:'tomb-mummy-proximity-burst',when:v=>v.world==='Tomb of the Ancients'&&v.lifecycle.alive&&v.game_ms>=near.state.view.time_ms+250});
  const burst=await save('natural-proximity-suicide',approaching),born=burst.map.entities.filter(e=>e.kind===0xd1b&&e.tomb?.producer===mummy.id&&!originalScarabs.has(e.id));
  observer.check('unattacked Mummy proximity suicide leaves five to eight additional owned Scarabs',!burst.map.entities.some(e=>e.id===mummy.id)&&born.length>=5&&born.length<=8&&burst.state.diagnostic.character.shots===0);
  observer.check('the removed Mummy still has natural burst projectiles in the native neighborhood',burst.state.view.projectiles.some(p=>p.owner===mummy.id));
  await approaching.act({keys:['Insert'],duration_ms:300});
  const returned=await approaching.inspect();
  observer.check('the same living proximity observer escapes without a Tomb clear',returned.view.map.name==='Nexus'&&returned.view.session.character===near.state.view.session.character&&returned.view.session.account===near.state.view.session.account&&returned.view.lifecycle.alive&&returned.diagnostic.character.tombs===0);
  await writeFile(join(out,'mummy-scope.json'),JSON.stringify({suppliedGearAndPositions:true,privilegedReadOnlyPlanning:true,weaponShots:0,npcEdits:false,protection:false,extraScarabs:born.map(e=>e.id),world,limits:'Prepared ordinary two-Mummy room lifecycle only; neighboring Mummies and their Scarabs remain intact. Not earned traversal, all phases, exact historical cadence, or a full Tomb clear.'},null,2));
}
