import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

const mode=process.env.CASTLE_MODE??'route';
const positions={route:'spawn',left:{x:127.5,y:39.5},right:{x:127.5,y:39.5},tower:{x:127.5,y:61.5},armor:{x:114.5,y:39.5},ordinary:{x:94.5,y:110.5}};
assert(Object.hasOwn(positions,mode),'Unknown CASTLE_MODE');
export const fixture={class:0x307,level:20,seed:866,world:'oryx-castle',position:positions[mode],equipment:[0xc02,0xb28,-1,0xac5],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50]};
export async function run(game){
  const root=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status==='succeeded',JSON.stringify(s));return s;};
  const map=async()=>{const v=await game.observe(),r=await fetch(config.origin+'/dev/map?player='+v.session.player,{headers,signal:AbortSignal.timeout(10000)});assert(r.ok);return r.json();};
  const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y),samples=[];
  async function save(name){const state=await game.inspect();samples.push({name,state});await writeFile(join(root,'castle-journey.json'),JSON.stringify(samples));console.log(name,JSON.stringify(state.view.position));return state;}
  let until=0,boostUntil=0;
  async function act(input,capture){
    const v=await game.observe();
    if(v.game_ms>=boostUntil){await api({op:'cheat',actor:game.actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}});boostUntil=(await game.observe()).game_ms+60000;}
    if(v.game_ms>=until){await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});until=(await game.observe()).game_ms+60000;}
    input.duration_ms=Math.min(input.duration_ms,until-(await game.observe()).game_ms);
    assert(input.duration_ms>0);
    return game.act(input,capture);
  }
  async function walk(point,tolerance=.45){
    for(let n=0;n<100;n++){
      const v=await game.observe(),dx=point.x-v.position.x,dy=point.y-v.position.y;if(Math.hypot(dx,dy)<tolerance)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[Math.abs(x)>.12&&Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>.12&&Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean);
      await act({keys,duration_ms:Math.max(50,Math.min(350,Math.round(Math.hypot(dx,dy)/(4+5.6*v.character.stats.speed/75)*1000)))});
    }
    throw Error('Native walk stalled '+JSON.stringify(point));
  }
  async function kill(id){
    const before=await game.inspect(),original=(await map()).entities.find(e=>e.id===id);assert(original);
    for(let n=0;n<200;n++){
      const m=await map(),e=m.entities.find(e=>e.id===id);
      if(!e){
        const after=await game.inspect();
        assert(after.diagnostic.character.kills>before.diagnostic.character.kills&&(original.expires===null||after.view.time_ms<original.expires),'Target disappeared without a credited native defeat before expiry');
        await writeFile(join(root,'defeat-'+id+'.json'),JSON.stringify({original,before,after}));return;
      }
      const v=await game.observe();
      const moving=[0xd78,0xd79,0xd7c].includes(e.kind);
      if(distance(v.position,e.pos)>6){await walk([0xd78,0xd79].includes(e.kind)?e.home:e.pos,2);continue;}
      if(moving){
        if(v.character.mp<75)await api({op:'cheat',actor:game.actor,change:{kind:'vitals',hp:v.character.hp,mp:v.character.stats.max_mp}});
        const lead=(distance(v.position,e.pos)/14+.1)/.05,dx=e.pos.x+e.last_step.x*lead-v.position.x,dy=e.pos.y+e.last_step.y*lead-v.position.y,x=50*(dx-dy)/Math.SQRT2,y=50*(dx+dy)/Math.SQRT2,scale=Math.max(1,Math.hypot(x,y)/270);
        await act({keys:[' '],duration_ms:700,mouse:{x:300+x/scale,y:325+y/scale}});
      }else await act({keys:[],duration_ms:350,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    }
    throw Error('Native damage did not defeat '+id);
  }
  function path(m,from,to){
    const w=m.width,key=p=>Math.floor(p.y)*w+Math.floor(p.x),start=key(from),end=key(to),blocked=new Set(m.blocked.map(([x,y])=>y*w+x));
    for(const e of m.entities)if([0xd70,0xd71].includes(e.kind))blocked.delete(key(e.pos));
    const previous=new Map([[start,null]]),queue=[start];
    for(let i=0;i<queue.length&&!previous.has(end);i++)for(const next of [queue[i]-w,queue[i]+w,queue[i]-1,queue[i]+1])if(next>=0&&next<w*m.height&&!blocked.has(next)&&!previous.has(next)){previous.set(next,queue[i]);queue.push(next);}
    assert(previous.has(end),'No Castle route to '+JSON.stringify(to));
    const route=[];for(let at=end;at!==start;at=previous.get(at))route.push({x:at%w+.5,y:Math.floor(at/w)+.5});return route.reverse();
  }
  const initial=await save('entrance'),authored=await map();
  await writeFile(join(root,'castle-authored-map.json'),JSON.stringify(authored));
  if(mode==='tower'){
    await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});
    await game.act({keys:[],duration_ms:1000},{name:'castle-quiet',when:()=>true});
    const hit=await save('tower-hit');
    game.check('natural Living Floor projectile applies Quiet and drains MP',!!(hit.view.character.conditions&2)&&hit.view.character.mp===0);
    game.check('selected tower does not add the later-guide Weak condition',!(hit.view.character.conditions&4)&&hit.view.lifecycle.alive);
    return;
  }
  await act({keys:[],duration_ms:1500},{name:'castle-entrance',when:()=>true});
  if(mode==='armor'||mode==='ordinary'){
    const before=await save('armor-before'),population=await map();
    const armor=population.entities.filter(e=>e.kind===0xd7e&&(mode==='armor'?e.parent!==null:e.parent===null)).sort((a,b)=>distance(a.pos,before.view.position)-distance(b.pos,before.view.position))[0];
    assert(armor&&distance(armor.pos,before.view.position)<5,'Requested authored/spawned armor absent');
    await kill(armor.id);
    const first=await save('armor-defeated');
    game.check('actual '+mode+' Haunted Armor defeat grants no XP or base fame',!((await map()).entities.some(e=>e.id===armor.id))&&first.view.character.experience===before.view.character.experience&&first.diagnostic.character.fame===before.diagnostic.character.fame);
    if(mode==='armor'){
      await act({keys:[],duration_ms:4000});
      const replacement=(await map()).entities.find(e=>e.kind===0xd7e&&e.parent===armor.parent&&e.id!==armor.id);assert(replacement,'Same spawner did not replace its actually defeated armor');
      await kill(replacement.id);
      const second=await save('replacement-defeated');
      game.check('same spawner replacement has a fresh identity and still grants no XP',!((await map()).entities.some(e=>e.id===replacement.id))&&second.view.character.experience===first.view.character.experience);
      await writeFile(join(root,'castle-armor-identities.json'),JSON.stringify({armor,replacement}));
    }
    return;
  }
  const goal={x:127.5,y:47.5},walls=[];
  if(mode==='route'){
  // Build 122 requires the full anti-rush route, not an arena-only fixture: https://wikiwiki.jp/rmd/version/build122.0.1
  for(let n=0;n<450;n++){
    const v=await game.observe();if(distance(v.position,goal)<.7)break;
    const m=await map(),route=path(m,v.position,goal);assert(route.length);
    const obstacle=route.map(p=>m.entities.find(e=>[0xd70,0xd71].includes(e.kind)&&distance(e.pos,p)<.2)).find(Boolean);
    if(obstacle&&distance(v.position,obstacle.pos)<6){await kill(obstacle.id);walls.push(obstacle);continue;}
    let end=0;const dx=route[0].x-Math.floor(v.position.x)-.5,dy=route[0].y-Math.floor(v.position.y)-.5;
    while(end+1<route.length&&end<5&&route[end+1].x-route[end].x===dx&&route[end+1].y-route[end].y===dy&&!m.entities.some(e=>[0xd70,0xd71].includes(e.kind)&&distance(e.pos,route[end+1])<.2))end++;
    const enemy=m.entities.filter(e=>e.kind>=0xd7e&&e.kind<=0xd87&&distance(e.pos,v.position)<5).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position))[0];
    if(enemy)await act({keys:[],duration_ms:250,target:{entity:enemy.id,camera_angle:-Math.PI/4,fire:true}});
    await walk(route[end]);
    if(n%10===0)await save('route-'+n);
  }
  const arena=await save('guardian-entrance');
  game.check('native Castle entrance-to-guardian route traversed without relocation',distance(arena.view.position,goal)<.7&&arena.view.session.character===initial.view.session.character&&walls.length>=3);
  game.check('ordinary population yields XP on full route',arena.view.character.experience>initial.view.character.experience&&arena.diagnostic.character.kills>initial.diagnostic.character.kills);
  await writeFile(join(root,'castle-walls.json'),JSON.stringify(walls));
  await act({keys:[],duration_ms:1500},{name:'castle-route',when:()=>true});
  return;
  }
  await walk({x:127.5,y:37.5});
  await act({keys:[],duration_ms:500});
  const idle=await save('idle-guardians'),pair=authored.entities.filter(e=>[0xd78,0xd79].includes(e.kind));
  assert(pair.length===2);
  game.check('both authored guardians are initially invulnerable outside activation range',pair.every(e=>idle.diagnostic.entities.find(x=>x.id===e.id)?.condition&(1<<24)));
  await walk({x:127.5,y:33.5});
  await act({keys:[],duration_ms:1000},{name:'castle-guardians',when:()=>true});
  const active=await save('active-guardians');
  game.check('approaching activates both actual guardians',pair.every(e=>!(active.diagnostic.entities.find(x=>x.id===e.id)?.condition&(1<<24))));
  const first=pair.find(e=>e.kind===(mode==='left'?0xd79:0xd78)),second=pair.find(e=>e.id!==first.id);
  await kill(first.id);
  const survived=await save('first-guardian-defeated');
  game.check('first actual defeat shields the living peer without unlocking Chamber',!survived.diagnostic.entities.some(e=>e.id===first.id)&&!!(survived.diagnostic.entities.find(e=>e.id===second.id)?.condition&(1<<24))&&!(await map()).entities.some(e=>e.kind===0xd7b));
  await act({keys:[],duration_ms:5000});
  const recovered=await save('bound-sword'),sword=(await map()).entities.find(e=>e.kind===0xd7c&&e.parent===second.id);assert(sword,'Survivor did not bind its sword');
  await act({keys:[],duration_ms:1500},{name:'castle-sword',when:()=>true});
  game.check('survivor binds exactly one real sword',(await map()).entities.filter(e=>e.kind===0xd7c).length===1);
  await kill(sword.id);
  await act({keys:[],duration_ms:1600});
  const released=await save('sword-defeated');
  game.check('actual bound sword defeat releases survivor vulnerability',!(await map()).entities.some(e=>e.id===sword.id)&&!(released.diagnostic.entities.find(e=>e.id===second.id)?.condition&(1<<24)));
  await kill(second.id);
  await act({keys:[],duration_ms:1500});
  const defeated=await save('pair-defeated'),cleared=await map(),portals=cleared.entities.filter(e=>e.kind===0xd7b);
  game.check('both actual guardian defeats earn exactly one Chamber portal',pair.every(e=>!cleared.entities.some(x=>x.id===e.id))&&portals.length===1&&defeated.view.character.experience>initial.view.character.experience);
  const bag=cleared.entities.find(e=>e.items.includes(0xa20)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===defeated.view.session.account));assert(bag,'Eligible Defense reward absent');
  await walk(bag.pos);
  await act({keys:[],duration_ms:800},{name:'castle-reward',when:()=>true});
  const slot=bag.items.indexOf(0xa20),before=await game.observe();
  await act({keys:[],duration_ms:350,drag:{from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to:{x:634,y:428}}});
  const picked=await save('defense-picked');
  game.check('ordinary loot drag commits earned Defense once',picked.view.character.items[4]===0xa20&&picked.view.character.inventory_revision===before.character.inventory_revision+1);
  const peer=await game.join({...fixture,world:initial.view.session.world,position:portals[0].pos});
  await walk(portals[0].pos);
  await act({keys:[],duration_ms:700},{name:'castle-chamber-portal',when:()=>true});
  await act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  for(let n=0;n<12&&(await game.observe()).session.world===initial.view.session.world;n++)await act({keys:[],duration_ms:500});
  const entered=await save('chamber-admitted');
  await peer.act({keys:[],duration_ms:700});
  await peer.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  for(let n=0;n<12&&(await peer.observe()).session.world===initial.view.session.world;n++)await peer.act({keys:[],duration_ms:500});
  const joined=await peer.observe();
  game.check('earned portal admits both living characters to one shared Chamber',entered.view.map.name==="Oryx's Chamber"&&entered.view.session.world===joined.session.world&&entered.view.session.character===initial.view.session.character&&entered.view.character.items[4]===0xa20&&entered.view.lifecycle.alive&&joined.lifecycle.alive);
  await act({keys:['d'],duration_ms:300});
  const final=await save('chamber-movement');
  game.check('same character moves after Chamber admission with stable guardian XP',distance(final.view.position,entered.view.position)>.1&&final.view.character.experience===defeated.view.character.experience);
  await writeFile(join(root,'castle-guardian-outcome.json'),JSON.stringify({mode,first,second,sword,bag,portal:portals[0],recovered,joined}));
}
