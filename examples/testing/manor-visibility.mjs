import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const scene = Object.freeze({
  name:'Manor of the Immortals', seed:12351, kind:0x181a,
  wall:Object.freeze({x:78.5,y:98.5}), outside:Object.freeze({x:79.5,y:98.5}),
  interior:Object.freeze({x:77,y:98}), maxActiveMs:22000,
});
const at = (a,b)=>a.x===b.x&&a.y===b.y;
const discovered = view=>view.tiles.some(t=>at(t,scene.interior));
let preparedWallId;

export function startPosition(info) {
  assert.equal(info.name,scene.name,'Wrong prepared dungeon');
  assert.equal(info.prepared_seed,String(scene.seed),'Wrong prepared layout seed');
  const walls=info.encounter_markers.filter(e=>e.kind===scene.kind&&at(e.position,scene.wall));
  assert.equal(walls.length,1,'Exact original bookshelf missing or ambiguous');
  assert.equal(walls[0].region,250,'Bookshelf is not a vulnerable secret-room door');
  assert.equal(walls[0].hp,3000,'Bookshelf must start at its ordinary full HP');
  assert(Number.isSafeInteger(walls[0].id)&&walls[0].id>0,'Prepared original wall ID missing');
  preparedWallId=walls[0].id;
  return {...scene.outside};
}

export const fixture = {
  class:0x30e, level:20, seed:1384, world:'manor-of-the-immortals', worldSeed:scene.seed,
  position:startPosition, equipment:[0xb08,-1,-1,-1], backpack:Array(8).fill(-1), stats:null,
};

export function checkState(state, first) {
  const v=state.view, b=first.view;
  assert.equal(v.map.name,scene.name);
  assert.equal(v.session.world,b.session.world);
  assert.equal(v.session.character,b.session.character);
  assert(v.lifecycle.alive&&!v.lifecycle.death_pending&&!v.lifecycle.departing&&!v.lifecycle.quarantined,'Native character terminal or quarantined');
  assert.equal(v.lifecycle.output,'live');
  assert(at(v.position,scene.outside),'Stationary visibility case moved');
  assert.deepEqual(v.character.items,b.character.items,'No inventory or loot changes permitted');
  assert.equal(state.diagnostic.character.manors,0,'Visibility case must not earn Manor completion');
  assert(v.time_ms>=b.time_ms&&v.time_ms-b.time_ms<=scene.maxActiveMs,'Actual active-game budget exceeded');
}

export function checkActionBudget(elapsed, duration, reserve=0) {
  assert(Number.isSafeInteger(elapsed)&&elapsed>=0);
  assert(Number.isSafeInteger(duration)&&duration>0&&duration<=1000);
  assert(Number.isSafeInteger(reserve)&&reserve>=0);
  assert(elapsed+duration+reserve<=scene.maxActiveMs,'No action may exceed the original 22-second budget');
}

function wallSnapshot(first,state,wall) {
  assert.deepEqual(state.view.session,first.view.session,'Actor/account/player/world binding changed');
  const d=state.diagnostic, ch=d.character, session=state.view.session;
  for(const field of ['world','player','account','character'])assert(Number.isSafeInteger(session[field])&&session[field]>0,'Missing session '+field);
  assert.equal(d.player,session.player,'Diagnostic player is not the native player');
  assert.equal(ch.id,session.character,'Hit metrics belong to another character');
  assert.equal(ch.account,session.account,'Hit metrics belong to another account');
  assert(Array.isArray(d.players)&&d.players.length===0,'Foreign nearby player present');
  assert(Array.isArray(state.view.player_markers)&&state.view.player_markers.length===0,'Foreign world player marker present');
  assert(Array.isArray(d.own_projectiles),'Own projectile observation missing');
  assert(ch.lifetime&&typeof ch.lifetime==='object','Lifetime hit metrics missing');
  assert.equal(ch.lifetime.version,1);
  assert.equal(ch.lifetime.completeness,'complete');
  const metrics={shots:ch.shots,hits:ch.hits,projectileHits:ch.lifetime.projectile_hits};
  for(const [name,value] of Object.entries(metrics))assert(Number.isSafeInteger(value)&&value>=0,'Missing or invalid '+name);
  assert.equal(ch.lifetime.shots,metrics.shots,'Admitted shot counters disagree');
  assert(metrics.projectileHits<=metrics.shots,'Accepted projectile hits exceed admitted shots');
  const targets=d.entities.filter(e=>e.id===wall.id);
  assert(targets.length<=1,'Duplicate original wall ID');
  assert(d.entities.filter(e=>e.kind===scene.kind&&at(e.pos,scene.wall)).every(e=>e.id===wall.id),'Replacement bookshelf at original position');
  assert(state.view.objects.filter(e=>(e.kind===scene.kind||e.blocks)&&at(e.position,scene.wall)).every(e=>e.id===wall.id),'Replacement wall at original position');
  if(!targets.length) {
    assert(!state.view.objects.some(e=>e.id===wall.id),'Diagnostic disappearance disagrees with visible wall');
    return {...metrics,hp:null,ownerDamage:null};
  }
  const target=targets[0];
  assert.equal(target.kind,scene.kind,'Original wall kind changed');
  assert(at(target.pos,scene.wall),'Original wall moved');
  assert.equal(target.condition,0,'Original wall became protected');
  assert.equal(target.expires,null,'Original wall has an expiry or missing expiry observation');
  assert.equal(target.parent,null,'Original wall is parent-owned or parent observation missing');
  assert(Number.isSafeInteger(target.hp)&&target.hp>0&&target.hp<=3000,'Invalid original wall HP');
  assert(target.damage&&typeof target.damage==='object'&&!Array.isArray(target.damage),'Target damage attribution missing');
  const ownerDamage=Object.entries(target.damage).reduce((sum,[actor,amount])=>{
    assert.equal(actor,String(session.character),'Original wall damage credited to another actor');
    assert(Number.isSafeInteger(amount)&&amount>=0,'Invalid original-wall damage credit');
    return sum+amount;
  },0);
  assert.equal(ownerDamage,3000-target.hp,'Original-wall HP loss lacks matching actor damage credit');
  return {...metrics,hp:target.hp,ownerDamage};
}

export function startWallEvidence(first,wall,preparedId,actor) {
  assert(Number.isSafeInteger(actor)&&actor>0&&actor===first.view.session.character,'Wrong native actor');
  assert(Number.isSafeInteger(preparedId)&&preparedId>0&&wall.id===preparedId,'Wall differs from pre-registration original ID');
  const snapshot=wallSnapshot(first,first,wall);
  assert.equal(snapshot.hp,3000,'Original wall must start at 3000 HP');
  return {...snapshot,sawNonfinalDamage:false,removed:false};
}

export function observeWallEvidence(first,state,wall,previous,input) {
  const next=wallSnapshot(first,state,wall);
  for(const metric of ['shots','hits','projectileHits'])assert(next[metric]>=previous[metric],'Weapon metric regressed: '+metric);
  const firing=input.target?.entity===wall.id&&input.target.fire===true&&input.keys.length===0;
  if(previous.removed) {
    assert.equal(next.hp,null,'Original wall reappeared');
    return {...next,sawNonfinalDamage:true,removed:true};
  }
  if(next.hp===null) {
    assert(firing&&previous.sawNonfinalDamage,'Premature disappearance without observed nonfinal targeted damage');
    assert(next.hits>previous.hits&&next.projectileHits>previous.projectileHits,'Disappearance lacks accepted projectile-hit advancement');
    return {...next,sawNonfinalDamage:true,removed:true};
  }
  assert(next.hp<=previous.hp,'Original wall HP increased');
  if(next.hp<previous.hp) {
    assert(firing,'Original-wall damage outside ordinary targeted staff input');
    assert(next.shots>previous.shots&&next.hits>previous.hits&&next.projectileHits>previous.projectileHits,'HP damage lacks admitted shots and accepted projectile hits');
    assert.equal(next.ownerDamage-previous.ownerDamage,previous.hp-next.hp);
  } else {
    assert.equal(next.hits,previous.hits,'Accepted damage without original-wall HP loss');
    assert.equal(next.projectileHits,previous.projectileHits,'Accepted projectile hit without original-wall HP loss');
  }
  return {...next,sawNonfinalDamage:previous.sawNonfinalDamage||next.hp<previous.hp,removed:false};
}

export async function run(game) {
  const first=await game.inspect(), samples=[{name:'arrival',state:first}];
  checkState(first,first);
  assert.equal(first.view.character.class,fixture.class);
  assert.equal(first.view.character.level,fixture.level);
  assert.deepEqual(first.view.character.items.slice(0,4),fixture.equipment);
  assert(first.view.character.items.slice(4).every(kind=>kind===-1));
  assert.equal(first.view.character.conditions,0,'Fresh Wizard must have no prepared protection');
  const walls=first.view.objects.filter(e=>e.kind===scene.kind&&at(e.position,scene.wall));
  assert.equal(walls.length,1,'Original unbroken bookshelf must be client-known');
  const wall=walls[0];
  assert.equal(wall.hp,3000);
  assert.equal(wall.max_hp,3000);
  const original=first.diagnostic.entities.find(e=>e.id===wall.id);
  assert(original&&original.condition===0,'Bookshelf is protected or absent');
  let current=first, requestedMs=0, weaponEvidence=startWallEvidence(first,wall,preparedWallId,game.actor);
  const save=()=>writeFile(join(process.env.QA_RESULTS_DIR,'manor-visibility-observations.json'),JSON.stringify({provenance:'prepared',requestedMs,weaponEvidence,causalScope:'Observed original-wall HP loss and surviving per-character target damage credit during targeted staff input; aggregate accepted-hit counters, not an exact final target-hit receipt.',samples},null,2));
  async function act(name,input,capture,reserve=0) {
    checkActionBudget(current.view.time_ms-first.view.time_ms,input.duration_ms,reserve);
    assert(requestedMs+input.duration_ms<=scene.maxActiveMs);
    requestedMs+=input.duration_ms;
    await game.act(input,capture?{name,when:()=>true}:undefined);
    current=await game.inspect();samples.push({name,state:current});await save();
    checkState(current,first);
    weaponEvidence=observeWallEvidence(first,current,wall,weaponEvidence,input);
    samples.at(-1).weaponEvidence=weaponEvidence;await save();
  }
  await save();
  await act('manor-intact-bookshelf',{keys:[],duration_ms:1000},true,1000);
  game.check('intact bookshelf withholds the undiscovered interior floor',!discovered(first.view)&&!discovered(current.view));
  assert(current.view.objects.some(e=>e.id===wall.id&&e.hp===3000),'Idle observation altered the bookshelf');
  for(let step=0;step<40&&current.diagnostic.entities.some(e=>e.id===wall.id);step++) {
    await act('staff-'+step,{keys:[],duration_ms:500,target:{entity:wall.id,camera_angle:0,fire:true}},false,1000);
  }
  assert(weaponEvidence.sawNonfinalDamage&&weaponEvidence.removed,'Original bookshelf did not complete the observed targeted-damage lifecycle within budget');
  await act('manor-open-bookshelf',{keys:[],duration_ms:1000},true);
  game.check('opened bookshelf reveals interior floor without movement',discovered(current.view)&&!current.view.objects.some(e=>e.id===wall.id));
  await save();
}
