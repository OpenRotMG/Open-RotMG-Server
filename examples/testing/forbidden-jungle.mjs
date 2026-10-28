import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

const part=process.env.JUNGLE_PART;
assert(['foliage','camp','basilisk','totem'].includes(part),'Set JUNGLE_PART to foliage, camp, basilisk or totem');
const out=process.argv[process.argv.indexOf('--run-dir')+1];
let center;
export const fixture={class:0x310,level:20,seed:12351,world:'forbidden-jungle',worldSeed:12351,
  position(){
    const events=readFileSync(join(out,'server.log'),'utf8').split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l));
    const generated=events.findLast(e=>e.fields?.message==='generated branching Forbidden Jungle').fields;
    const rooms=JSON.parse(generated.layout.replaceAll('(','[').replaceAll(')',']'));
    const index=part==='foliage'?0:rooms.findIndex((r,i)=>i>0&&i!==generated.boss_room&&i%3==={camp:0,totem:1,basilisk:2}[part]);
    assert(index>=0);const r=rooms[index];center={x:20+r[0]*20+.5,y:20+r[1]*20+.5};
    return {x:center.x,y:center.y+4};
  },equipment:[0xa3d,0xb25,0xb05,0xacd],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,75]};

export async function run(game){
  const trace=[];
  const save=async label=>{const s=await game.inspect();trace.push({label,...s});await writeFile(join(out,'observations.json'),JSON.stringify(trace));return s;};
  const entity=(s,id)=>s.diagnostic.entities.find(e=>e.id===id);
  const near=(s,kind)=>s.diagnostic.entities.filter(e=>e.kind===kind).sort((a,b)=>Math.hypot(a.pos.x-center.x,a.pos.y-center.y)-Math.hypot(b.pos.x-center.x,b.pos.y-center.y))[0];
  const keys=(a,b)=>{const dx=b.x-a.x,dy=b.y-a.y,x=dx-dy,y=dx+dy;return [Math.abs(x)>.12&&Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>.12&&Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean);};
  async function walk(point){
    for(let n=0;n<65;n++){
      const v=await game.observe(),d=Math.hypot(v.position.x-point.x,v.position.y-point.y);if(d<.35)return;
      await game.act({keys:keys(v.position,point),duration_ms:Math.max(50,Math.min(200,Math.round(d*100)))});
    }
    assert.fail('Native movement did not reach '+JSON.stringify(point));
  }
  async function shoot(id,duration_ms){await game.act({keys:[],duration_ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});}
  async function defeat(id){
    for(let n=0;n<45;n++){if(!entity(await game.inspect(),id))return;await shoot(id,350);await game.act({keys:[],duration_ms:150});}
    assert.fail('Native shots did not defeat '+id);
  }
  async function capture(name){await game.act({keys:[],duration_ms:700},{name,when:()=>true});}
  async function heal(){const v=await game.observe();if(v.character.hp<550&&v.character.mp>=90&&!(v.character.conditions&2))await game.act({keys:[' '],duration_ms:350});}
  const start=await save('arrival');await capture(part+'-arrival');
  if(part==='foliage'){
    const report=JSON.parse(readFileSync(join(out,'result.json'),'utf8'));
    const token=readFileSync(join(out,'state/control.token'),'utf8').trim();
    const response=await fetch(report.origin+'/dev/map?player='+start.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
    const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
    const clear=e=>{
      const a=start.view.position,d=Math.hypot(e.pos.x-a.x,e.pos.y-a.y);
      return d<8&&d>1&&Array.from({length:Math.ceil((d-.8)*10)},(_,i)=>{const t=i/10/d;return !blocked.has([Math.floor(a.x+(e.pos.x-a.x)*t),Math.floor(a.y+(e.pos.y-a.y)*t)].join(','));}).every(Boolean);
    };
    // Similar plant art has different collision/HP contracts in the selected definitions: https://github.com/OpenRotMG/Open-RotMG-Server/blob/dd4191abe3dbefb488689750997145acb52a09f2/data/objects.xml
    const decoration=start.diagnostic.entities.find(e=>e.kind>=0xdd4&&e.kind<=0xdd7&&clear(e));assert(decoration,'No clear decorative plant');
    await walk(decoration.pos);const decorated=await save('decoration-crossed');
    game.check('nonblocking decorative foliage remains while walked through',Boolean(entity(decorated,decoration.id))&&Math.floor(decorated.view.position.x)===Math.floor(decoration.pos.x)&&Math.floor(decorated.view.position.y)===Math.floor(decoration.pos.y));
    await walk(start.view.position);
    const plant=start.diagnostic.entities.find(e=>e.kind===0xdd8&&clear(e));assert(plant,'No clear breakable plant');
    const permanent=start.diagnostic.entities.find(e=>[0xdc9,0xdca,0xdcb,0xdd9,0xdda].includes(e.kind)&&clear(e));assert(permanent,'No clear permanent foliage');
    const position=(await game.observe()).position,dx=permanent.pos.x-position.x,dy=permanent.pos.y-position.y;
    await game.act({keys:[],duration_ms:2500,mouse:{x:300+35.355*(dx-dy),y:325+35.355*(dx+dy)}});const intact=await save('permanent-shot');
    game.check('native weapon fire leaves permanent foliage intact',Boolean(entity(intact,permanent.id)));
    await capture('permanent-foliage');
    await shoot(plant.id,3000);const opened=await save('plant-destroyed');
    game.check('native weapon destroys only the selected 500-HP plant',!entity(opened,plant.id)&&Boolean(entity(opened,permanent.id)));
    await walk(plant.pos);const crossed=await save('plant-crossed');
    game.check('ordinary native movement crosses the destroyed plant tile',Math.floor(crossed.view.position.x)===Math.floor(plant.pos.x)&&Math.floor(crossed.view.position.y)===Math.floor(plant.pos.y));
    await capture('opened-foliage-path');
  }else if(part==='camp'){
    const fire=near(start,0xdd3);assert(fire);
    const tribe=start.diagnostic.entities.filter(e=>e.parent===fire.id);assert.equal(tribe.length,5);
    game.check('generated camp binds all five tribes to its own Fire',tribe.filter(e=>e.kind===0xdd0).length===2);
    const hunter=tribe.find(e=>e.kind===0xdd2);assert(hunter);
    let previous;
    for(let n=0;n<12;n++){
      await shoot(hunter.id,50);await game.act({keys:[],duration_ms:250});previous=await save('tribe-shot');
      if(entity(previous,hunter.id)?.hp<300)break;
    }
    let healed=false,conditions=false,orbit=false;
    game.check('ordinary wand shot wounds a living tribe member',entity(previous,hunter.id)?.hp>0&&entity(previous,hunter.id).hp<300);
    for(let n=0;n<24;n++){
      await game.act({keys:[],duration_ms:250});const s=await save('camp-sample');
      const a=entity(previous,hunter.id),b=entity(s,hunter.id);if(a&&b&&b.hp>a.hp)healed=true;
      conditions||=(s.view.character.conditions&258)===258;
      orbit||=tribe.some(e=>{const p=entity(s,e.id);return p&&p.parent===fire.id&&Math.hypot(p.pos.x-e.pos.x,p.pos.y-e.pos.y)>.2;});
      previous=s;await heal();
    }
    game.check('wounded tribe receives observed healing',healed);
    game.check('Shaman stars apply selected Hallucinating and Quiet',conditions);
    game.check('owned tribes move around their Fire',orbit);await capture('tribal-healing');
    let beforeKill;
    for(let n=0;n<45;n++){
      const s=await game.inspect();if(!entity(s,fire.id))break;beforeKill=s;
      await shoot(fire.id,50);await game.act({keys:[],duration_ms:850});await heal();
    }
    assert(beforeKill);trace.push({label:'before-final-fire-shot',...beforeKill});
    const survivors=tribe.filter(e=>entity(beforeKill,e.id)),detached=await save('fire-defeated');
    game.check('Fire defeat detaches surviving tribes without deleting them',!entity(detached,fire.id)&&survivors.length>0&&survivors.every(e=>entity(detached,e.id)?.parent===null));
    await game.act({keys:[],duration_ms:2500});const orphan=await save('tribes-orphaned');
    game.check('detached tribes survive the timed ownership sweep',survivors.every(e=>entity(orphan,e.id)?.parent===null));
    await capture('surviving-tribes');
  }else{
    const kind=part==='totem'?0xdcd:0xdcf,childKind=part==='totem'?0xdcc:0xdce;
    const parent=near(start,kind);assert(parent);await game.act({keys:[],duration_ms:2200});
    const active=await save('family-active'),children=active.diagnostic.entities.filter(e=>e.parent===parent.id&&e.kind===childKind);
    game.check('generated '+part+' has its bounded owned family',children.length===(part==='totem'?3:2));
    game.check(part+' projectiles reach the native client',active.view.projectiles.some(p=>p.owner===parent.id));
    await capture(part+'-family');await defeat(parent.id);const detached=await save('parent-defeated');
    game.check(part+' defeat detaches living children',children.every(e=>entity(detached,e.id)?.parent===null));
    await game.act({keys:[],duration_ms:4500});const survivors=await save('family-survives');
    game.check(part+' children survive without stale ownership',children.every(e=>entity(survivors,e.id)?.parent===null));
    game.check(part+' parent does not respawn or keep summoning',!entity(survivors,parent.id)&&!survivors.diagnostic.entities.some(e=>e.parent===parent.id));
    await capture(part+'-survivors');
  }
  const end=await save('final');
  game.check('same supplied living Priest finishes '+part,end.view.lifecycle.alive&&end.view.session.character===start.view.session.character);
  game.check('ordinary-room checks grant no dungeon clear',end.diagnostic.character.jungles===0);
}
