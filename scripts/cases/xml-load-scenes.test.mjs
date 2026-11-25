import {test} from 'node:test';
import assert from 'node:assert/strict';
import {summarizeScene} from './xml-load-scenes.mjs';

const start=Date.parse('2026-11-12T00:00:00Z');
function window(seconds,world=7){
  const timestamp=new Date(start+seconds*1000).toISOString();
  return [
    {timestamp,fields:{message:'world performance',world_id:world,actors:8,entities:100,bullets:2,humans:1,bots:0,timings:JSON.stringify({achieved_step_hz:19.9,timings:{step:{p95_us:800,p99_us:1800}},thread_cpu:{step:{p95_us:700}}})}},
    {timestamp,fields:{message:'directory health',memory:JSON.stringify({resident_kib:100000})}},
    {timestamp,fields:{message:'host snapshot',host:JSON.stringify({game_cores:.2})}},
  ];
}
test('xml-load-scenes excludes setup/boundary and other-world windows and fixes numeric budgets',()=>{
  const events=[...window(2),...window(7,8),...window(10),...window(15),...window(21)];
  const {metrics,budgets}=summarizeScene(events,start,start+20000,7);
  assert.equal(metrics.windows,2);
  assert.deepEqual(budgets,{step_hz_min:18.9,step_p95_us:1600,step_p99_us:3600,step_cpu_p95_us:1400,game_cores_max:.30000000000000004,resident_kib_max:157768,actors_max:12,entities_max:110,bullets_max:6});
});
test('xml-load-scenes rejects missing observations rather than reporting success',()=>{
  assert.throws(()=>summarizeScene(window(10),start,start+20000,7),/two complete/);
  const rows=[...window(10),...window(15)];
  for(const host of ['{}','{"game_cores":null}']){
    rows.find(r=>r.fields.message==='host snapshot').fields.host=host;
    assert.throws(()=>summarizeScene(rows,start,start+20000,7),/Missing numeric/);
  }
});
