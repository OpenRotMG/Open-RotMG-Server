import assert from 'node:assert/strict';
import {cp, readFile, writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {fixture, sthenoJourney, SOURCE} from './stheno.mjs';
import {medusaDraw, prepareAbyssApproach, abyssJourney} from './abyss-earned-entry.mjs';
export const realmSeed=106;
const worldSeed=107n;

let portalFixture;
export async function prepare(api) {
  assert.equal(medusaDraw(121604,worldSeed).toFixed(6),'0.017711','independent reference matches the pinned prior draw');
  portalFixture=await prepareAbyssApproach(api,worldSeed,draw=>draw>=.04&&draw<.5);
  return fixture;
}

// Run the same retained executable with --data for each variant; XML_AUTHORITY_VARIANT names which one was selected.
const ACTORS = 'game/123.5.1/actors.xml';
// Exact edits inside the Stheno object (actors.xml:3046-3120) with their expected occurrence counts.
const EDITS = {
  changed: [
    // cooldown: attack1 Shoot 0.7s -> 0.2s (both shielded and unshielded children)
    ['projectileId="0" cooldown="0.7" type="targeted" predictive="0.15"', 'projectileId="0" cooldown="0.2" type="targeted" predictive="0.15"', 4],
    // spawn: Stheno Pet maxChildren 4 -> 2
    ['childIds="Stheno Pet" cooldown="1.5" maxChildren="4"', 'childIds="Stheno Pet" cooldown="1.5" maxChildren="2"', 1],
    // state: chance_to_clear_snakes returns to attack1 instead of attack2
    ['<Transition afterTime="5">attack2</Transition>', '<Transition afterTime="5">attack1</Transition>', 1],
    // reward: Snake Eye Ring row prob .10 -> 1
    ['<SoulboundLoot prob=".10" threshold=".10">Snake Eye Ring</SoulboundLoot>', '<SoulboundLoot prob="1" threshold=".10">Snake Eye Ring</SoulboundLoot>', 1],
  ],
  // A malformed number must stop startup with file/line/object context.
  invalid: [['childIds="Stheno Pet" cooldown="1.5"', 'childIds="Stheno Pet" cooldown="1.5s"', 1]],
};
// portal declaration: Medusa -> Abyss of Demons .04 -> .5 (actors.xml:1585), applied inside the Medusa object.
const PORTAL = {type: '0x0654', from: '<Behavior prob=".04" childId="Abyss of Demons Portal">', to: '<Behavior prob=".5" childId="Abyss of Demons Portal">'};
const CHANGED = {
  ...SOURCE,
  order: ['wait_for_pets', 'attack1', 'summon_snakes', 'chance_to_clear_snakes', 'attack1'],
  pets: 2,
  // 6 shots every 0.2s for 8s: roughly 240.
  attack1Shots: [150, 330],
  ring: {probability: 1, guaranteed: true},
};
function object(text, type = '0x0917') {
  const start = text.indexOf(`<Object type="${type}"`), end = text.indexOf('</Object>', start);
  assert(start >= 0 && end > start, `object ${type} missing`);
  return [start, end];
}
/** Copies a complete data directory and applies one variant's exact Stheno edits. */
export async function writeVariant(source, target, variant) {
  await cp(source, target, {recursive: true, errorOnExist: true, force: false});
  if (variant === 'source') return;
  const path = join(target, ACTORS);
  let text = await readFile(path, 'utf8');
  const edit = (type, edits) => {
    const [start, end] = object(text, type);
    let block = text.slice(start, end);
    for (const [from, to, count] of edits) {
      assert.equal(block.split(from).length - 1, count, from);
      block = block.split(from).join(to);
    }
    text = text.slice(0, start) + block + text.slice(end);
  };
  edit('0x0917', EDITS[variant]);
  if (variant === 'changed') edit(PORTAL.type, [[PORTAL.from, PORTAL.to, 1]]);
  await writeFile(path, text);
}
export async function run(observer) {
  const variant = process.env.XML_AUTHORITY_VARIANT;
  assert(['source', 'changed'].includes(variant), 'Set XML_AUTHORITY_VARIANT=source|changed');
  const root = process.env.QA_RESULTS_DIR, config = JSON.parse(await readFile(join(root, 'native/native.json'), 'utf8'));
  const environment = (await readFile(join(config.state, 'server.log'), 'utf8')).split('\n').filter(l => l.includes('"local environment selected"')).map(l => JSON.parse(l).fields);
  assert.equal(environment.length, 1, 'one server environment');
  const actors = await readFile(join(environment[0].data, ACTORS), 'utf8'), block = actors.slice(...object(actors));
  observer.check(`loaded data is the ${variant} variant`, EDITS.changed.every(([from, to]) => block.includes(variant === 'changed' ? to : from) && !block.includes(variant === 'changed' ? from : to)));
  const rates = await (await fetch(config.origin + '/api/drop-rates', {signal: AbortSignal.timeout(10000)})).json();
  const portal = rates.objects.find(o => o.id === Number(PORTAL.type)).death_births.find(b => b.target.name === 'Abyss of Demons Portal');
  observer.check(`advertised Medusa portal chance follows the ${variant} XML under the same binary`, portal.compiled_behavior.fields.Prob.Number === (variant === 'changed' ? 0.5 : 0.04));
  // The Realm traveller must not wait in combat while the Snake Pit case advances the shared clock.
  await sthenoJourney(observer, variant === 'changed' ? CHANGED : SOURCE);
  const {medusa,...spec}=portalFixture, draw=medusaDraw(medusa,worldSeed), expected=variant==='changed';
  await writeFile(join(root,'authority-portal-draw.json'),JSON.stringify({variant,realmSeed,medusa,draw,expected,source:.04,changed:.5}));
  const traveller=await observer.join(spec);
  await abyssJourney(traveller,medusa,expected);
}
