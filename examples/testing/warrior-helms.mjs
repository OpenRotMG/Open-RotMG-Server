import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const out=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x31d,level:20,seed:767,world:-2,position:'spawn',equipment:[0xa00,0xa66,0xa14,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
const has=(view,index)=>Boolean(view.character.conditions&(1<<(index-1)));
export async function run(game){
  const initial=await game.observe(),{x,y}=initial.position;
  const general=await game.join({...fixture,seed:768,position:{x:x+4,y},equipment:[0xa00,0xb29,0xa14,-1]});
  const juggernaut=await game.join({...fixture,seed:769,position:{x:x+4.25,y},equipment:[0xa00,0xc08,0xa14,-1]});
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
  await cast(game);
  [self]=await snapshot('cooldown-refusal');
  game.check('early native retry neither spends mana nor advances cooldown',self.character.mp>=mp&&self.character.ability_ready_ms===ready);
  await idle(3100);
  [self,edge]=await snapshot('combat-expired');
  game.check('Combat Helm effects expire on caster and boundary ally',!has(self,20)&&!has(self,15)&&!has(edge,20));
  const beforeGeneral=await general.observe();
  await cast(general);
  [self,edge,outside]=await snapshot('general');
  game.check('Great General spends 85 MP and adds only its own Speedy',beforeGeneral.character.mp-edge.character.mp>=84&&beforeGeneral.character.mp-edge.character.mp<=85&&has(edge,15)&&has(self,20)&&!has(self,15)&&has(outside,20)&&!has(outside,15));
  await cast(game);
  await idle(3300);
  [self]=await snapshot('shorter-overlap');
  game.check('shorter Combat recast does not truncate existing six-second Berserk',has(self,20)&&!has(self,15));
  await idle(2800);
  [self,edge,outside]=await snapshot('general-expired');
  game.check('longer aura eventually expires on all three players',!has(self,20)&&!has(edge,20)&&!has(outside,20));
  const beforeJugg=await juggernaut.observe();
  await juggernaut.act({keys:[' '],duration_ms:1500},{name:'juggernaut',when:v=>has(v,26)&&has(v,20)});
  [self,edge,outside]=await snapshot('juggernaut');
  game.check('Juggernaut spends 90 MP and grants Armored instead of Speedy',beforeJugg.character.mp-outside.character.mp>=88&&beforeJugg.character.mp-outside.character.mp<=90&&has(outside,26)&&!has(outside,15)&&has(outside,20));
  game.check('Juggernaut aura reaches nearby ally but Armored stays self-only',has(edge,20)&&!has(edge,26)&&!has(self,20)&&!has(self,26));
  await idle(2000);
  outside=await juggernaut.observe();
  game.check('Juggernaut Armored remains active before 4.5 seconds',has(outside,26));
  await idle(1800);
  [self,edge,outside]=await snapshot('juggernaut-expired');
  game.check('Juggernaut effects expire after 4.5 seconds',!has(outside,26)&&!has(outside,20)&&!has(edge,20));
  for(let n=0;n<2;n++){await cast(game);await idle(4300);}
  [self]=await snapshot('mana-refusal');
  game.check('four Combat activations leave less than one cast of mana',self.character.mp<60);
  const lowMp=self.character.mp,lowReady=self.character.ability_ready_ms;
  await cast(game);
  [self]=await snapshot('insufficient-mana');
  game.check('insufficient mana leaves cooldown unchanged and applies no buff',self.character.mp>=lowMp&&self.character.ability_ready_ms===lowReady&&!has(self,20)&&!has(self,15));
  for(const actor of [game,general,juggernaut]){const view=await actor.observe();game.check('same living Warrior retains supplied equipment',view.lifecycle.alive&&view.character.class===0x31d&&view.character.items[0]===0xa00&&view.character.items[2]===0xa14);}
}
