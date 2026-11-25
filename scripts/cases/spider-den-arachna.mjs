import {fixture as approach,arachnaCase} from './spider-exit.mjs';
export const fixture={...approach,equipment:[0xb08,-1,0xb05,0xac5],backpack:Array(8).fill(-1)};
export const run=game=>arachnaCase(game,true);
