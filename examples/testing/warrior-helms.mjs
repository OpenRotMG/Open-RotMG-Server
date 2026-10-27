import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const out=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x31d,level:20,seed:767,world:-2,position:'spawn',equipment:[0xa00,0xa66,0xa7b,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
const has=(view,index)=>Boolean(view.character.conditions&(1<<(index-1)));
export async function run(game){
  const initial=await game.observe(),{x,y}=initial.position;
  const general=await game.join({...fixture,seed:768,position:{x:x+4,y},equipment:[0xa00,0xb29,0xa7b,-1]});
  const juggernaut=await game.join({...fixture,seed:769,position:{x:x+4.25,y},equipment:[0xa00,0xc08,0xa7b,-1]});
  const records=[];
  const snapshot=async label=>{const views=await Promise.all([game.observe(),general.observe(),juggernaut.observe()]);records.push({label,views});await writeFile(join(out,'helm-observations.json'),JSON.stringify(records,null,2));return views;};
  const cast=actor=>actor.act({keys:[' '],duration_ms:250});
  const idle=ms=>game.act({keys:[],duration_ms:ms});
  await snapshot('prepared');
  // Later helm rebalances must not replace the selected definitions: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf
  await game.act({keys:[' '],duration_ms:1500},{name:'combat-helm',when:v=>has(v,20)&&has(v,15)});
  let [self,edge,outside]=await snapshot('combat');
  game.check('Combat Helm spends exactly 60 MP before at most two points of regeneration',initial.character.mp-self.character.mp>=58&&initial.character.mp-self.character.mp<=60);
  game.check('caster receives Berserk and Speedy',has(self,20)&&has(self,15));
  game.check('ally exactly four tiles away receives only Berserk',has(edge,20)&&!has(edge,15)&&Math.abs(edge.position.x-self.position.x-4)<0.001);
  game.check('ally beyond four tiles receives neither effect',!has(outside,20)&&!has(outside,15));
  const ready=self.character.ability_ready_ms,mp=self.character.mp;
  const retry=(await cast(game)).steps[0];
  await snapshot('cooldown-refusal');
  game.check('observed retry inside cooldown spends no additional mana',retry.game_ms<ready&&retry.character.mp>=mp);
  await idle(3100);
  [self,edge]=await snapshot('combat-expired');
  game.check('Combat Helm effects expire on caster and boundary ally',!has(self,20)&&!has(self,15)&&!has(edge,20));
  const step=(actor,keys,duration_ms)=>({actor:actor.actor,input:{keys,duration_ms}});
  const beforeGeneral=await general.observe();
  const overlap=await game.act([[step(general,[' '],250),step(game,[],250),step(juggernaut,[],250)],step(game,[' '],250),step(game,[],3300)]);
  const boost=overlap.steps.find(s=>s.actor===general.actor),party=overlap.steps.filter(s=>s.actor===game.actor),neighbor=overlap.steps.find(s=>s.actor===juggernaut.actor);
  await snapshot('shorter-overlap');
  game.check('Great General spends 85 MP and adds only its own Speedy',beforeGeneral.character.mp-boost.character.mp>=84&&beforeGeneral.character.mp-boost.character.mp<=85&&has(boost,15)&&has(party[0],20)&&!has(party[0],15)&&has(neighbor,20)&&!has(neighbor,15));
  game.check('shorter Combat recast does not truncate existing six-second Berserk',has(party[1],15)&&has(party[2],20)&&!has(party[2],15));
  game.check('Great General equipment adds selected defense and regeneration stats',boost.character.stats.defense===40&&boost.character.stats.vitality===77&&boost.character.stats.wisdom===2);
  await idle(2800);
  [self,edge,outside]=await snapshot('general-expired');
  game.check('longer aura eventually expires on all three players',!has(self,20)&&!has(edge,20)&&!has(outside,20));
  const beforeJugg=await juggernaut.observe();
  const armored=await juggernaut.act([[step(juggernaut,[' '],250),step(general,[],250),step(game,[],250)],step(juggernaut,[],2000),step(juggernaut,[],3000)],{name:'juggernaut',when:v=>has(v,26)&&has(v,20)});
  const phases=armored.steps.filter(s=>s.actor===juggernaut.actor),near=armored.steps.find(s=>s.actor===general.actor),far=armored.steps.find(s=>s.actor===game.actor);
  const [start,during,end]=phases;
  game.check('Juggernaut spends 90 MP and grants Armored instead of Speedy',beforeJugg.character.mp-start.character.mp>=88&&beforeJugg.character.mp-start.character.mp<=90&&has(start,26)&&!has(start,15)&&has(start,20));
  game.check('Juggernaut aura reaches nearby ally but Armored stays self-only',has(near,20)&&!has(near,26)&&!has(far,20)&&!has(far,26));
  game.check('Juggernaut equipment adds selected defense and speed',start.character.stats.defense===40&&start.character.stats.speed===55);
  game.check('Juggernaut Armored remains active at an observed time before 4.5 seconds',during.game_ms-start.before_ms<4500&&has(during,26));
  game.check('Juggernaut effects expire at an observed time after 4.5 seconds',end.game_ms-start.game_ms>=4500&&!has(end,26)&&!has(end,20));
  await snapshot('juggernaut-expired');
  for(let n=0;n<2;n++){await cast(game);await idle(4300);}
  [self]=await snapshot('mana-refusal');
  game.check('four Combat activations leave less than one cast of mana',self.character.mp<60);
  const lowMp=self.character.mp;
  game.check('low-mana attempt starts after the previous cooldown',self.character.ability_ready_ms<=self.game_ms);
  await cast(game);
  [self]=await snapshot('insufficient-mana');
  game.check('insufficient mana starts no cooldown and applies no buff',self.character.mp>=lowMp&&self.character.ability_ready_ms<=self.game_ms&&!has(self,20)&&!has(self,15));
  for(const actor of [game,general,juggernaut]){const view=await actor.observe();game.check('same living Warrior retains supplied equipment',view.lifecycle.alive&&view.character.class===0x31d&&view.character.items[0]===0xa00&&view.character.items[2]===0xa7b);}
}
