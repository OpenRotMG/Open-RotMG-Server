import {fixture as start,runRoster} from './legacy-pets.mjs';
export const fixture={...start,world:-2};
export const run=game=>runRoster(game,2);
