import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

const root=process.env.QA_RESULTS_DIR;
const fixture={class:0x310,level:20,seed:791,world:-3,position:'spawn',equipment:[0xaf6,0xb25,0xb05,0xac5],backpack:[0x70b,-1,-1,-1,-1,-1,-1,-1],stats:[670,385,50,25,50,50,40,75]};
export async function prepare(){
  const native=join(root,'native'),{origin}=JSON.parse(await readFile(join(native,'result.json'),'utf8'));
  const token=(await readFile(join(native,'state','control.token'),'utf8')).trim();
  const response=await fetch(origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'travel-denial-capacity',command:{op:'prepare_realm_capacity',limit:1}}),signal:AbortSignal.timeout(10000)});
  const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  assert.equal(receipt.result.world,fixture.world);
  await writeFile(join(root,'capacity.json'),JSON.stringify(receipt));
  return fixture;
}
export async function run(game){
  const observations=[];
  function progress(view){const {ability_ready_ms,...character}=view.character;return character;}
  async function save(name,actor=game){const view=await actor.observe();observations.push({name,view});await writeFile(join(root,'denial.json'),JSON.stringify(observations,null,2));return view;}
  const initial=await save('realm');
  await game.act({keys:['1'],duration_ms:700});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  for(let i=0;i<8&&(await game.observe()).session.world===initial.session.world;i++)await game.act({keys:[],duration_ms:500});
  const dungeon=await save('key-dungeon');assert.equal(dungeon.world,'Snake Pit');
  const other=await game.join({...fixture,seed:792,backpack:Array(8).fill(-1)});
  const occupant=await save('full-realm-occupant',other);assert.equal(occupant.session.world,initial.session.world);
  await game.act({keys:[],duration_ms:700});
  const before=await save('before-refusal');
  // Refuse rather than reroute a full origin: project policy, not recovered 2012 rules. https://github.com/OpenRotMG/Open-RotMG-Server/pull/438
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1000});
  const denied=await save('after-refusal');
  const log=await readFile(join(root,'native','server.log'),'utf8');
  game.check('full-origin portal handler executes a recorded refusal',log.split('\n').filter(line=>line.startsWith('{')).map(line=>JSON.parse(line)).some(row=>row.fields?.message==='portal transfer refused: destination full'&&row.fields.player===before.session.player&&row.fields.destination===initial.session.world));
  game.check('refusal retains the same living source session and exact inventory',denied.lifecycle.alive&&JSON.stringify(denied.session)===JSON.stringify(before.session)&&JSON.stringify(progress(denied))===JSON.stringify(progress(before)));
  await game.act({keys:[],duration_ms:700},{name:'full-origin-refusal',when:v=>v.session.player===denied.session.player&&v.lifecycle.alive});
  await game.act({keys:['w'],duration_ms:300});
  const moved=await save('source-movement');
  game.check('refused source remains playable',moved.session.player===denied.session.player&&Math.hypot(moved.position.x-denied.position.x,moved.position.y-denied.position.y)>.1);
  const stayed=await save('occupant-unchanged',other);
  game.check('refusal leaves the other account in its own living Realm session',stayed.lifecycle.alive&&JSON.stringify(stayed.session)===JSON.stringify(occupant.session)&&JSON.stringify(progress(stayed))===JSON.stringify(progress(occupant)));
  game.check('prepared key stays consumed once after refusal',denied.character.items.slice(4).every(item=>item===-1)&&denied.character.inventory_revision===initial.character.inventory_revision+1);
}
