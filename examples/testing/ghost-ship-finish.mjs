import {prepare,run as ship} from './ghost-ship.mjs';
export {prepare};
export const run=game=>ship(game,{finishOnly:true});
