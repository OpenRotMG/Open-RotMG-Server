import assert from 'node:assert/strict';
import test from 'node:test';
import {scene,startWallEvidence,observeWallEvidence} from './manor-visibility.mjs';

const actor=101, wall={id:51,kind:scene.kind,position:{...scene.wall},hp:3000,max_hp:3000,blocks:true};
const firing={keys:[],duration_ms:500,target:{entity:wall.id,camera_angle:0,fire:true}};
const idle={keys:[],duration_ms:1000};
function sample(hp=3000,shots=0,hits=0,projectileHits=0) {
  const entity={id:wall.id,kind:scene.kind,pos:{...scene.wall},hp,condition:0,parent:null,expires:null,damage:hp===3000?{}:{[actor]:3000-hp}};
  return {
    view:{session:{world:2,player:9,account:11,character:actor},objects:hp>0?[{...wall,hp}]:[],player_markers:[]},
    diagnostic:{player:9,players:[],own_projectiles:[],character:{id:actor,account:11,shots,hits,lifetime:{version:1,completeness:'complete',shots,projectile_hits:projectileHits}},entities:hp>0?[entity]:[]},
  };
}
function begin() {
  const first=sample();return {first,evidence:startWallEvidence(first,wall,wall.id,actor)};
}
function damaged() {
  const {first,evidence}=begin();return {first,evidence:observeWallEvidence(first,sample(2900,2,1,1),wall,evidence,firing)};
}

test('new lifecycle guard accepts only sampled nonfinal own-target damage before removal',()=>{
  const {first,evidence}=begin();
  const missed=observeWallEvidence(first,sample(3000,2),wall,evidence,firing);
  assert.equal(missed.sawNonfinalDamage,false,'Shots alone are not hit evidence');
  const hit=observeWallEvidence(first,sample(2900,4,1,1),wall,missed,firing);
  assert.equal(hit.ownerDamage,100);assert.equal(hit.sawNonfinalDamage,true);assert.equal(hit.removed,false);
  const removed=observeWallEvidence(first,sample(0,6,3,3),wall,hit,firing);
  assert.equal(removed.removed,true);assert.equal(removed.hp,null);assert.equal(removed.ownerDamage,null);
  assert.equal(observeWallEvidence(first,sample(0,6,3,3),wall,removed,idle).removed,true);
});

test('premature disappearance fails even with shots and aggregate accepted hits',()=>{
  const {first,evidence}=begin();
  for(const input of [idle,firing])assert.throws(()=>observeWallEvidence(first,sample(0,2,1,1),wall,evidence,input),/Premature disappearance/);
  const shotsOnly=observeWallEvidence(first,sample(3000,2),wall,evidence,firing);
  assert.throws(()=>observeWallEvidence(first,sample(0,4,2,2),wall,shotsOnly,firing),/Premature disappearance/);
});

test('unchanged original HP cannot borrow accepted hits from other targets',()=>{
  const {first,evidence}=begin();
  assert.throws(()=>observeWallEvidence(first,sample(3000,2,1,1),wall,evidence,firing),/Accepted damage without original-wall HP loss/);
  assert.throws(()=>observeWallEvidence(first,sample(3000,2,0,1),wall,evidence,firing),/Accepted projectile hit without original-wall HP loss/);
});

test('original wall binds pre-registration ID and rejects replacement, moved or wrong-kind walls',()=>{
  const {first,evidence}=damaged();
  assert.throws(()=>startWallEvidence(sample(),wall,52,actor),/pre-registration original ID/);
  assert.throws(()=>startWallEvidence(sample(),wall,undefined,actor),/pre-registration original ID/);
  for(const mutate of [
    s=>{s.diagnostic.entities[0].id=52;s.view.objects[0].id=52;},
    s=>{s.diagnostic.entities[0].kind=0x1826;s.diagnostic.entities[0].id=52;s.view.objects[0].kind=0x1826;s.view.objects[0].id=52;},
    s=>s.diagnostic.entities[0].kind=0x1826,
    s=>s.diagnostic.entities[0].pos.x++,
  ]){
    const s=sample(2800,4,2,2);mutate(s);assert.throws(()=>observeWallEvidence(first,s,wall,evidence,firing));
  }
});

test('wrong actor/account/player/world and foreign players cannot supply weapon metrics',()=>{
  const {first,evidence}=begin();
  assert.throws(()=>startWallEvidence(first,wall,wall.id,102),/Wrong native actor/);
  for(const mutate of [s=>s.diagnostic.character.id++,s=>s.diagnostic.character.account++,s=>s.diagnostic.player++,s=>s.view.session.world++,s=>s.view.session.player++,s=>s.view.session.character++,s=>s.view.session.account++,s=>s.diagnostic.players.push({id:77}),s=>s.view.player_markers.push({id:77}),s=>s.diagnostic.entities[0].damage={102:100}]){
    const s=sample(2900,2,1,1);mutate(s);assert.throws(()=>observeWallEvidence(first,s,wall,evidence,firing));
  }
});

test('missing or invalid DTO hit metrics, credit, roster or projectile fields fail closed',()=>{
  const {first,evidence}=begin();
  for(const mutate of [s=>delete s.diagnostic.character.hits,s=>delete s.diagnostic.character.lifetime.projectile_hits,s=>delete s.diagnostic.character.lifetime,s=>s.diagnostic.character.lifetime.completeness='known_only',s=>delete s.diagnostic.character.lifetime.shots,s=>delete s.diagnostic.entities[0].damage,s=>delete s.diagnostic.entities[0].parent,s=>delete s.diagnostic.entities[0].expires,s=>s.diagnostic.entities[0].expires=20000,s=>s.diagnostic.entities[0].parent=77,s=>delete s.diagnostic.own_projectiles,s=>delete s.diagnostic.players,s=>delete s.view.player_markers,s=>{delete s.view.session.account;delete s.diagnostic.character.account;},s=>s.diagnostic.character.hits=NaN,s=>s.diagnostic.character.lifetime.projectile_hits=3,s=>s.diagnostic.entities[0].damage={[actor]:'100'}]){
    const s=sample(2900,2,1,1);mutate(s);assert.throws(()=>observeWallEvidence(first,s,wall,evidence,firing));
  }
});

test('nonfinal HP damage requires both real accepted-hit counters and own target damage credit during staff fire',()=>{
  const {first,evidence}=begin();
  for(const s of [sample(2900,2,0,0),sample(2900,2,1,0),sample(2900,2,0,1),sample(2900,0,1,0)])assert.throws(()=>observeWallEvidence(first,s,wall,evidence,firing));
  const uncredited=sample(2900,2,1,1);uncredited.diagnostic.entities[0].damage={};
  assert.throws(()=>observeWallEvidence(first,uncredited,wall,evidence,firing),/HP loss lacks matching actor damage credit/);
  assert.throws(()=>observeWallEvidence(first,sample(2900,2,1,1),wall,evidence,idle),/outside ordinary targeted staff input/);
  assert.throws(()=>observeWallEvidence(first,sample(2900,2,1,1),wall,evidence,{...firing,target:{...firing.target,entity:52}}),/outside ordinary targeted staff input/);
});

test('post-damage disappearance requires targeted fire and fresh accepted-hit advancement, not fired shots alone',()=>{
  const {first,evidence}=damaged();
  assert.throws(()=>observeWallEvidence(first,sample(0,4,2,2),wall,evidence,idle),/Premature disappearance/);
  assert.throws(()=>observeWallEvidence(first,sample(0,4,1,1),wall,evidence,firing),/accepted projectile-hit advancement/);
  assert.throws(()=>observeWallEvidence(first,sample(0,4,2,1),wall,evidence,firing),/accepted projectile-hit advancement/);
  const removed=observeWallEvidence(first,sample(0,4,2,2),wall,evidence,firing);
  assert.throws(()=>observeWallEvidence(first,sample(3000,4,2,2),wall,removed,idle),/reappeared/);
});
