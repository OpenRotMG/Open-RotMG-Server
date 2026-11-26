import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

export const realmSeed=12351;
export const fixture={class:0x30e,level:1,seed:12351,world:-1,position:{x:12.5,y:8.5},equipment:[0xa97,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
const duration=Number(process.env.XML_LOAD_SAMPLE_MS??20000);
assert(Number.isInteger(duration)&&duration>=20000&&duration<=60000,'XML_LOAD_SAMPLE_MS must be 20000–60000');
const root=process.env.QA_RESULTS_DIR,scenes=[];
let primary=true;
const save=async(name,value)=>writeFile(join(root,name),JSON.stringify(value,null,2)+'\n');

// Exclude boundary windows: their counters may include setup, pauses or the previous scene.
export function summarizeScene(events,start,end,world){
  const rows=events.filter(e=>Date.parse(e.timestamp)>=start+6000&&Date.parse(e.timestamp)<=end).map(e=>e.fields);
  const worlds=rows.filter(f=>f.message==='world performance'&&f.world_id===world);
  const directory=rows.filter(f=>f.message==='directory health');
  const hosts=rows.filter(f=>f.message==='host snapshot');
  assert(worlds.length>=2&&directory.length>=2&&hosts.length>=2,'Need two complete world/directory/host windows per scene');
  const timings=worlds.map(f=>JSON.parse(f.timings));
  const memory=directory.map(f=>JSON.parse(f.memory));
  const host=hosts.map(f=>JSON.parse(f.host));
  const numeric=values=>{assert(values.length&&values.every(Number.isFinite),'Missing numeric baseline metric');return values;};
  const max=values=>Math.max(...numeric(values)),min=values=>Math.min(...numeric(values));
  const metrics={windows:worlds.length,step_hz_min:min(timings.map(t=>t.achieved_step_hz)),step_p95_us:max(timings.map(t=>t.timings.step.p95_us)),step_p99_us:max(timings.map(t=>t.timings.step.p99_us)),step_cpu_p95_us:max(timings.map(t=>t.thread_cpu.step.p95_us)),game_cores_max:max(host.map(h=>h.game_cores)),resident_kib_max:max(memory.map(m=>m.resident_kib)),actors_max:max(worlds.map(f=>f.actors)),entities_max:max(worlds.map(f=>f.entities)),bullets_max:max(worlds.map(f=>f.bullets)),humans_max:max(worlds.map(f=>f.humans)),bots_max:max(worlds.map(f=>f.bots))};
  assert(Object.values(metrics).every(Number.isFinite),'Missing numeric baseline metric');
  const budgets={step_hz_min:Math.max(15,metrics.step_hz_min-1),step_p95_us:Math.max(1000,Math.ceil(metrics.step_p95_us*2)),step_p99_us:Math.max(2000,Math.ceil(metrics.step_p99_us*2)),step_cpu_p95_us:Math.max(1000,Math.ceil(metrics.step_cpu_p95_us*2)),game_cores_max:Math.max(.1,metrics.game_cores_max*1.5),resident_kib_max:Math.ceil(metrics.resident_kib_max*1.25+32768)};
  for(const key of ['actors_max','entities_max','bullets_max'])budgets[key]=metrics[key]+Math.max(4,Math.ceil(metrics[key]*.1));
  return {metrics,budgets,windows:worlds,host_windows:hosts,directory_windows:directory};
}
async function events(){
  const lines=(await readFile(join(root,'native/server.log'),'utf8')).split('\n').slice(0,-1);
  return lines.filter(l=>l.startsWith('{')).map(l=>JSON.parse(l));
}
async function record(name,world,work,context){
  const start=Date.now();await work();const end=Date.now();
  const measured=summarizeScene(await events(),start,end,world);
  if(context.population_samples)for(const key of ['summons','loot_bags']){
    const maximum=Math.max(...context.population_samples.map(s=>s[key]));
    measured.metrics[key+'_max']=maximum;measured.budgets[key+'_max']=maximum+4;
  }
  scenes.push({name,world,start:new Date(start).toISOString(),end:new Date(end).toISOString(),duration_ms:duration,context,...measured});
  await save('xml-load-scenes.json',{schema:1,realm_seed:realmSeed,fixture,prepared:true,policy:'Same isolated seeded scenes only; exclude first 6s. Budget: cadence baseline minus 1Hz (floor 15Hz); p95 2x/floor 1ms; p99 2x/floor 2ms; CPU cores 1.5x/floor .1; RSS 1.25x +32MiB; actor/entity/projectile maxima +10%/floor 4. Not a production capacity claim.',scenes});
}
export async function beforeNavigate(){
  if(!primary)return;primary=false;
  const config=JSON.parse(await readFile(join(root,'native/result.json'),'utf8'));
  const token=(await readFile(join(root,'native/state/control.token'),'utf8')).trim();
  let serial=0;
  const api=async command=>{
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'xml-load-'+(++serial),command}),signal:AbortSignal.timeout(10000)});
    const value=await response.json();assert(response.ok&&value.status!=='failed',JSON.stringify(value));
    if(command.op==='world_info')return value;
    assert.equal(value.status,'succeeded',JSON.stringify(value));return value.result;
  };
  await record('empty',-2,async()=>{await api({op:'resume'});try{await delay(duration);}finally{await api({op:'pause',native_paused:[]});}},{description:'Nexus without connected players; scenery/merchants/portals remain. Server still owns seeded dormant Realm and Tutorial.',native_participants:0});
  const {world}=await api({op:'world',template:'snake-pit',seed:12351});
  const info=await api({op:'world_info',world});
  await record('dormant',world,async()=>{await api({op:'resume'});try{await delay(duration);}finally{await api({op:'pause',native_paused:[]});}},{description:'Seeded generated Snake Pit without any admitted player; not a zero-entity world.',native_participants:0,info});
}
export async function run(game){
  const before=await game.inspect();
  game.check('seeded baseline completes actual native LOAD alive in Tutorial',before.view.map.name==='Tutorial'&&before.view.lifecycle.alive);
  const sample=async(name,participants)=>{
    const context={native_participants:participants,prepared:true,initial:await game.inspect(),population_samples:[]};
    await record(name,before.view.session.world,async()=>{
      for(let elapsed=0;elapsed<duration;elapsed+=5000){
        await game.act({keys:[],duration_ms:Math.min(5000,duration-elapsed)},elapsed===5000?{name:'xml-load-'+name,when:()=>true}:undefined);
        const s=await game.inspect(),entities=s.diagnostic.entities;
        context.population_samples.push({game_ms:s.view.time_ms,entities:entities.length,living:entities.filter(e=>e.hp>0).length,summons:entities.filter(e=>e.parent!==null).length,loot_bags:entities.filter(e=>e.items.length>0).length});
      }
    },context);
    const scene=scenes.at(-1);
    game.check(name+' observes the requested native population',scene.metrics.humans_max===participants);
  };
  await sample('populated-solo',1);
  const peer=await game.join({...fixture,seed:12352,position:{x:13.5,y:8.5}});
  const pair=await Promise.all([game.inspect(),peer.inspect()]);
  game.check('multiplayer baseline has distinct accounts in the same world',pair[0].view.session.account!==pair[1].view.session.account&&pair[0].view.session.world===pair[1].view.session.world);
  await sample('populated-multiplayer',2);
  const final=await Promise.all([game.inspect(),peer.inspect()]);
  game.check('both native participants remain alive and see each other',final.every((s,i)=>s.view.lifecycle.alive&&s.view.objects.some(o=>o.id===final[1-i].view.session.player)));
  game.check('all four scenes provide numeric diagnostic budgets',scenes.length===4&&scenes.every(s=>s.metrics.windows>=2&&s.metrics.bots_max===0));
  await save('xml-load-final.json',{before,final});
}
