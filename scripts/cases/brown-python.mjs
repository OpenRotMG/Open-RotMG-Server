import {fixture as base,checkPythons} from './pythons.mjs';
export const fixture={...base,position:{x:86.5,y:49.5}};
export const run=game=>checkPythons(game,[0x226],fixture.position);
