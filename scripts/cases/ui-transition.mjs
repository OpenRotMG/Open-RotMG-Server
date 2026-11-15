import { readFile } from 'node:fs/promises';
import { setTimeout as wait } from 'node:timers/promises';

// Supply the CDP call function for your owned QA tab; this example never launches or adopts a browser.
export async function exampleNexusReturn(call, serverLog, character) {
  const log = await readFile(serverLog, 'utf8');
  const before = log.slice(0, log.lastIndexOf('\n') + 1);
  // The logger may be halfway through its next write; only complete lines are evidence.
  const entries = text => text.split('\n').slice(0, -1).filter(line => line.startsWith('{'))
    .map(line => JSON.parse(line))
    .filter(row => row.fields?.message === 'player entered development world'
      && row.fields.character === character);
  const initial = entries(before).at(-1);
  if (!initial || typeof initial.fields.world !== 'string' || initial.fields.world === 'Nexus') {
    throw new Error('Start this example with the owned character in a non-Nexus world');
  }
  const key = { key: 'Insert', code: 'Insert', windowsVirtualKeyCode: 45 };
  let failure;
  try {
    // CDP page focus and DOM focus do not activate the user's desktop window.
    await call('Emulation.setFocusEmulationEnabled', {enabled:true});
    const bounds = await call('Runtime.evaluate', {
      expression: `JSON.stringify((() => {
        const player = document.querySelector('ruffle-player');
        if (!player) throw Error('Ruffle game not found');
        player.focus({preventScroll:true});
        if (document.hidden || !document.hasFocus() || document.activeElement !== player)
          throw Error('Owned Ruffle game did not acquire page-local focus; use an isolated display');
        const r = player.getBoundingClientRect();
        if (r.width <= 0 || r.height <= 0 || r.left < 0 || r.top < 0 || r.right > innerWidth || r.bottom > innerHeight)
          throw Error('Game lies outside input viewport');
        return {x:r.x, y:r.y, width:r.width, height:r.height, scale:1};
      })())`, returnByValue: true,
    });
    if (bounds.exceptionDetails) throw new Error(JSON.stringify(bounds.exceptionDetails));
    const clip = JSON.parse(bounds.result.value);
    await call('Input.dispatchKeyEvent', { type: 'keyDown', ...key });
    await call('Page.captureScreenshot', {format:'png', clip});
    await wait(80);
    await call('Input.dispatchKeyEvent', { type: 'keyUp', ...key });
    const deadline = Date.now() + 30000;
    while (Date.now() < deadline) {
      await call('Page.captureScreenshot', {format:'png', clip});
      const current = await readFile(serverLog, 'utf8');
      if (!current.startsWith(before)) throw new Error('Server log rotated during the transition');
      const entered = entries(current.slice(before.length)).at(-1);
      if (entered) {
        if (entered.fields.world !== 'Nexus') throw new Error('Character entered the wrong world');
        console.log(JSON.stringify(entered.fields));
        return entered.fields;
      }
      await wait(100);
    }
    throw new Error(`Insert dispatched for character ${character} in ${initial.fields.world}, but no new Nexus entry was observed`);
  } catch (error) {
    failure = error;
    throw error;
  } finally {
    const cleanupErrors = [];
    for (const [method, params] of [
      ['Input.dispatchKeyEvent', {type:'keyUp', ...key}],
      ['Emulation.setFocusEmulationEnabled', {enabled:false}],
    ]) {
      try { await call(method, params); } catch (error) { cleanupErrors.push(error); }
    }
    if (cleanupErrors.length) {
      if (!failure) throw new AggregateError(cleanupErrors, 'Native input cleanup failed');
      console.error('Native input cleanup also failed; original transition error retained:', cleanupErrors);
    }
  }
}
