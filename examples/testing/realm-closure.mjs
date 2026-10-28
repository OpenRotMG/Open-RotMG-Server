import assert from 'node:assert/strict';
import {appendFile,readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

const out=process.env.QA_RESULTS_DIR;
const spec={class:0x307,level:20,seed:787,world:1,equipment:[0xc02,0xade,0xaf9,0xacd],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50]};
const roots=new Set([0x668,0x67d,0x91f,0x91b,0x928,0x675,0x678,0xd56,0xd59,0xd5f,0xd50,0xd54,0xd61,0xe37]);
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),boss=world.encounter_markers.find(e=>e.kind===0x67d);
  assert(boss,'An ordinarily populated Cyclops encounter is required');
  return {...spec,position:boss.position};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'},protection=new Map(),records=[];
  let attackUntil=0,blocked,full,stalled=0;
  const fullKinds=new Set([...((await readFile('data/objects.xml','utf8')).matchAll(/<Object\s+type="([^"]+)"[^>]*>([\s\S]*?)<\/Object>/g))].filter(m=>m[2].includes('<FullOccupy')).map(m=>Number(m[1])));
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)}),value=await response.json();
    assert(response.ok&&value.status==='succeeded',JSON.stringify(value));return value;
  }
  const cheat=(client,change)=>api({op:'cheat',actor:client.actor,change});
  async function events(){return (await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);}
  async function map(){
    const response=await fetch(config.origin+'/dev/map?player='+(await game.observe()).session.player,{headers,signal:AbortSignal.timeout(30000)});
    assert(response.ok,'Privileged map observation failed');const value=await response.json();
    if(blocked){blocked.fill(0);full.fill(0);for(const [x,y] of value.blocked)blocked[y*value.width+x]=1;for(const e of value.entities)if(fullKinds.has(e.kind))full[Math.floor(e.pos.y)*value.width+Math.floor(e.pos.x)]=1;}
    return value;
  }
  async function save(label){const value={label,main:await game.inspect(),peer:await peer.observe()};records.push(value);await writeFile(join(out,'closure.json'),JSON.stringify(records));return value;}
  const initial=await game.observe(),origin=initial.session.world,peer=await game.join({...spec,seed:788,world:origin,position:initial.position}),peerInitial=await peer.observe();
  async function act(input,capture){
    let remaining=input.duration_ms,result;
    while(remaining>0){
      const now=(await game.observe()).game_ms;
      for(const client of [game,peer])if(now>=(protection.get(client.actor)??0)){
        await cheat(client,{kind:'protection',condition:'invincible',duration_ms:60000});protection.set(client.actor,now+60000);
      }
      if(now>=attackUntil){await cheat(game,{kind:'stat_modifier',index:2,amount:100,duration_ms:60000});attackUntil=now+60000;}
      const duration_ms=Math.min(remaining,...[...protection.values()].map(until=>until-now));
      assert(duration_ms>0,'Protection deadline must advance');
      result=await game.act({...input,duration_ms},capture);remaining-=duration_ms;capture=undefined;
    }
    return result;
  }
  const opening=await map(),width=opening.width,total=width*opening.height,original=opening.entities.filter(e=>roots.has(e.kind)&&(e.quest===e.id||[0x668,0x67d].includes(e.kind)));
  blocked=new Uint8Array(total);full=new Uint8Array(total);
  for(const [x,y] of opening.blocked)blocked[y*width+x]=1;
  for(const e of opening.entities)if(fullKinds.has(e.kind))full[Math.floor(e.pos.y)*width+Math.floor(e.pos.x)]=1;
  opening.blocked.length=0;opening.entities.length=0;
  const clear=(a,b)=>{
    const steps=Math.ceil(Math.hypot(b.x-a.x,b.y-a.y)*4);
    for(let n=0;n<=steps;n++){
      const px=a.x+(b.x-a.x)*n/Math.max(1,steps),py=a.y+(b.y-a.y)*n/Math.max(1,steps),x=Math.floor(px),y=Math.floor(py),dx=Math.sign(px-x-.5),dy=Math.sign(py-y-.5);
      if(x<0||x>=width||y<0||y>=opening.height||blocked[y*width+x])return false;
      for(const [nx,ny] of [[x+dx,y],[x,y+dy],[x+dx,y+dy]])if((nx!==x||ny!==y)&&full[ny*width+nx])return false;
    }
    return true;
  };
  async function walk(anchor,range){
    const before=await game.observe(),start=Math.floor(before.position.y)*width+Math.floor(before.position.x),parent=new Int32Array(total).fill(-1),queue=new Int32Array(total);
    parent[start]=start;queue[0]=start;let head=0,tail=1,goal;
    while(head<tail){
      const i=queue[head++],point={x:i%width+.5,y:Math.floor(i/width)+.5};
      if(Math.hypot(point.x-anchor.x,point.y-anchor.y)<range&&clear(point,anchor)){goal=i;break;}
      for(const n of [i-width,i-1,i+1,i+width])if(n>=0&&n<total&&Math.abs(n%width-i%width)<=1&&!blocked[n]&&parent[n]===-1){parent[n]=i;queue[tail++]=n;}
    }
    assert.notEqual(goal,undefined,'No walkable route to the living encounter');
    const path=[];for(let i=goal;i!==start;i=parent[i])path.push({x:i%width+.5,y:Math.floor(i/width)+.5});
    path.push({x:start%width+.5,y:Math.floor(start/width)+.5});
    const point=path.find(p=>{const dx=Math.abs(p.x-before.position.x),dy=Math.abs(p.y-before.position.y);return Math.hypot(dx,dy)<45&&Math.min(dx,dy,Math.abs(dx-dy))<.6&&clear(before.position,p);});
    assert(point,'No unobstructed next route segment: '+JSON.stringify({anchor,range,position:before.position,path:path.slice(-4)}));
    const dx=point.x-before.position.x,dy=point.y-before.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');
    if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
    const distance=keys.length===2?Math.min(Math.abs(x),Math.abs(y))*Math.SQRT2:Math.max(Math.abs(x),Math.abs(y));
    const duration_ms=Math.max(200,Math.min(5000,Math.round(distance*850/(4+5.6*before.character.stats.speed/75))));
    await act({keys,duration_ms});
    const after=await game.observe(),trace={anchor,range,point,keys,duration_ms,before:before.position,after:after.position,game_ms:after.game_ms};
    await appendFile(join(out,'routes.jsonl'),JSON.stringify(trace)+'\n');
    stalled=Math.hypot(after.position.x-before.position.x,after.position.y-before.position.y)>.02?0:stalled+1;
    assert(stalled<3,'Three stationary native route inputs: '+JSON.stringify(trace));
  }
  assert(original.length>20,'Full ordinary roster required, not a prepared final encounter');
  await writeFile(join(out,'opening.json'),JSON.stringify({original,initial,peerInitial}));
  await act({keys:[],duration_ms:500},{name:'realm-opening',when:()=>true});
  await save('opening');
  const seen=new Map(original.map(e=>[e.quest??e.id,e]));
  for(let encounter=0;encounter<100;encounter++){
    const log=await events(),credits=log.filter(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin),defeated=new Set(credits.map(e=>e.hero));
    const current=await map();
    for(const e of current.entities)if(roots.has(e.kind)&&e.quest===e.id)seen.set(e.id,e);
    current.blocked.length=0;current.entities.length=0;
    const position=(await game.observe()).position;
    const next=[...seen.keys()].filter(id=>!defeated.has(id)).sort((a,b)=>Math.hypot(seen.get(a).pos.x-position.x,seen.get(a).pos.y-position.y)-Math.hypot(seen.get(b).pos.x-position.x,seen.get(b).pos.y-position.y))[0];
    if(next===undefined){assert(credits.at(-1)?.remaining===0,'Every counted root must earn actual death credit');break;}
    let completed=false;
    for(let step=0;step<200;step++){
      const log=await events();if(log.some(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin&&e.hero===next)){completed=true;break;}
      const state=await game.inspect(),family=state.diagnostic.entities.filter(e=>e.id===next||e.quest===next);
      if(!family.length){
        if(Math.hypot(state.view.position.x-seen.get(next).pos.x,state.view.position.y-seen.get(next).pos.y)>15)await walk(seen.get(next).pos,12);
        else await act({keys:[],duration_ms:1000});
        continue;
      }
      const vulnerable=family.filter(e=>e.hp>0&&!(e.condition&((1<<24)|(1<<25))));
      const healer=vulnerable.find(e=>e.kind===0x91d);
      const target=healer??vulnerable.find(e=>roots.has(e.kind)||[0x920,0x91c,0x92d,0x6c1,0x929,0xd5e,0xd62].includes(e.kind));
      const anchor=target??family.find(e=>roots.has(e.kind)||[0x920,0x91c,0x92d,0x6c1,0x6c2].includes(e.kind));assert(anchor,'Encounter has no living leader or required combat target');
      const view=await game.observe();
      const range=anchor.kind===0xd5e?12:3;
      if(Math.hypot(anchor.pos.x-view.position.x,anchor.pos.y-view.position.y)>range||!clear(view.position,anchor.pos)){await walk(anchor.pos,range);continue;}
      if(target){
        const nearby=(await game.inspect()).view.objects.some(e=>e.id===target.id);assert(nearby,'Combat target must be visible to the native client');
        await act({keys:[],duration_ms:target.stage===2&&[0x91f,0x91b,0x928].includes(target.kind)?4000:2000,target:{entity:target.id,camera_angle:-Math.PI/4,fire:true}});
      }else await act({keys:[],duration_ms:anchor.stage===1&&[0x91f,0x91b,0x928].includes(anchor.kind)?1000:10000});
    }
    assert(completed||(await events()).some(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin&&e.hero===next),'Native combat did not finish counted encounter '+next);
    const receipt={root:next,kind:seen.get(next).kind,view:await game.observe()};
    await writeFile(join(out,'root-'+next+'.json'),JSON.stringify(receipt));
  }
  const log=await events(),credits=log.filter(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin);
  game.check('full ordinary roster plus admitted events earns once-only closure credit',credits.length===seen.size&&new Set(credits.map(e=>e.hero)).size===credits.length&&credits.at(-1)?.remaining===0);
  const closing=await save('closed');
  game.check('both original native characters remain in the closed Realm',closing.main.view.session.world===origin&&closing.peer.session.world===origin);
  await act({keys:[],duration_ms:700},{name:'realm-closed',when:()=>true});
  // Reconnect retains charId rather than creating a successor: https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/messaging/impl/GameServerConnection.as#L1635-L1690
  for(let n=0;n<30&&(await game.observe()).session.world===origin;n++)await act({keys:[],duration_ms:5000});
  const arrived=await save('castle');
  for(const [label,before,after,settled] of [['killer',initial,arrived.main.view,closing.main.view],['observer',peerInitial,arrived.peer,closing.peer]]){
    game.check(label+' follows the forced route into Castle with the same living character',after.world==="Oryx's Castle"&&after.lifecycle.alive&&after.session.account===before.session.account&&after.session.character===before.session.character&&after.session.world!==origin);
    game.check(label+' retains exact equipment and revision through closure',JSON.stringify(after.character.items)===JSON.stringify(before.character.items)&&after.character.inventory_revision===before.character.inventory_revision);
    game.check(label+' retains earned XP through closure',after.character.experience>=settled.character.experience);
  }
  game.check('collective recipients enter the same Castle instance',arrived.main.view.session.world===arrived.peer.session.world);
  game.check('observer never fired native shots',(await peer.inspect()).diagnostic.character.shots===0);
  await act({keys:[],duration_ms:3000},{name:'castle-arrival',when:v=>v.world==="Oryx's Castle"&&v.lifecycle.alive});
  const finalLog=await events(),route=finalLog.find(e=>e.message==='Realm closed; collective Castle route and fresh Realm installed'&&e.origin===origin),departure=finalLog.findIndex(e=>e.message==='all collective recipients durably departed; replacement delay started'&&e.origin===origin),published=finalLog.findIndex(e=>e.message==='safe collective departure completed; replacement published'&&e.origin===origin);
  game.check('normal replacement is published only after collective durable departure',route?.recipients===2&&departure>=0&&published>departure&&finalLog[published].replacement===route.replacement);
  await writeFile(join(out,'lifecycle.json'),JSON.stringify({original:[...seen.values()],credits,route,events:finalLog.filter(e=>e.origin===origin)}));
}
