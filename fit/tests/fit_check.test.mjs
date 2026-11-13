// Runs the three fit helpers in headless Chromium (musicindex ADR 0065, How This Decision
// Is Checked). Each fault page has one fault. The helper for that fault must give one
// finding, and the other helpers must give none. The two pages control-overflow and
// overlap come from the website at 761 px on 2026-11-13. The correct page must give no finding.
//
// The test sends each helper to the page as source text, as a walk script of an app does.
// It needs Node.js 22 or later and Chromium. Set CHROMIUM to the path of the browser
// when `chromium` is not on the PATH.
//
// Usage: node --test fit/tests/fit_check.test.mjs
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';

import { ALLOW_BREAK_ATTRIBUTE, edgeCuts, fitMessage, overflows, overlaps, wordBreaks } from '../web/fit-check.mjs';

const WIDTH = 400;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const style = `<style>
  * { box-sizing: border-box; }
  body { margin: 0; font: 16px/1.25 sans-serif; }
  .group { margin: 0 16px; padding: 16px; border: 1px solid; }
  .segmented { display: flex; gap: 8px; }
  .segment { display: block; white-space: nowrap; padding: 0 8px; }
</style>`;
const page = (body) => `<!doctype html><meta charset="utf-8">${style}<body>${body}</body>`;

const pages = {
  correct: page(`
    <div class="group">
      <p>A row of short words that wraps at the spaces between the words in the group.</p>
      <label>Name <input style="width: 150px"></label>
      <details><summary>More</summary><p>The text of a closed disclosure</p></details>
    </div>
    <p ${ALLOW_BREAK_ATTRIBUTE} style="overflow-wrap: anywhere">https://example.com/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</p>
    <div style="overflow-x: auto"><table style="width: 900px"><tr><td>A wide table in a scroll area</td></tr></table></div>
    <p>A self-titled release</p>
    <div class="segmented" role="radiogroup">
      <span class="segment" role="radio">Releases 20+</span><span class="segment" role="radio">Tracks</span>
    </div>
    <p><button>Back</button> <button disabled style="opacity: 0.4">Forward</button> <input placeholder="Search"></p>
    <dialog id="layer"><button>Close</button></dialog>`),
  'control-overflow': page(`
    <div class="segmented" role="radiogroup">
      <span class="segment" role="radio" style="width: 60px">Releases 20+</span><span class="segment" role="radio">Tracks</span>
    </div>`),
  overlap: page(`
    <div style="position: relative; height: 48px">
      <input placeholder="Search" style="position: absolute; left: 0; width: 200px">
      <button disabled style="position: absolute; left: 150px; opacity: 0.4">Forward</button>
    </div>`),
  'edge-cut': page(`
    <div class="group">
      <p>Text with inner space</p>
      <p style="margin-left: -24px">Cut text</p>
    </div>`),
  overflow: page(`
    <p>Text in the window</p>
    <div style="width: 600px"><span>A box that is wider than the window</span></div>`),
  'word-break': page(`
    <p>Text in the window</p>
    <p style="width: 40px; overflow-wrap: anywhere">Releases</p>`),
};

let chrome;
let profile;
let ws;
let id = 0;
const pending = new Map();
const waiters = [];

const send = (method, params = {}) => new Promise((resolve, reject) => {
  const n = ++id;
  pending.set(n, (msg) => (msg.error ? reject(new Error(`${method}: ${msg.error.message}`)) : resolve(msg.result)));
  ws.send(JSON.stringify({ id: n, method, params }));
});
const event = (method) => new Promise((resolve) => waiters.push({ method, resolve }));

const evaluate = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, returnByValue: true });
  if (r.exceptionDetails) throw new Error(`${expression.slice(0, 60)} -> ${JSON.stringify(r.exceptionDetails)}`);
  return r.result.value;
};

const open = async (html) => {
  const loaded = event('Page.loadEventFired');
  await send('Page.navigate', { url: `data:text/html;charset=utf-8,${encodeURIComponent(html)}` });
  await loaded;
};

const run = async () => ({
  'edge-cut': await evaluate(`(${edgeCuts})()`),
  overflow: await evaluate(`(${overflows})()`),
  overlap: await evaluate(`(${overlaps})()`),
  'word-break': await evaluate(`(${wordBreaks})()`),
});

before(async () => {
  profile = mkdtempSync(join(tmpdir(), 'fit-check-'));
  chrome = spawn(process.env.CHROMIUM || 'chromium', [
    '--headless=new', '--no-sandbox', '--disable-gpu', '--disable-crash-reporter', '--disable-breakpad',
    '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank',
  ], { stdio: 'ignore', env: { ...process.env, HOME: profile, XDG_CONFIG_HOME: profile, XDG_CACHE_HOME: profile } });
  let port;
  for (let i = 0; i < 100 && !port; i++) {
    await sleep(100);
    try { port = readFileSync(join(profile, 'DevToolsActivePort'), 'utf8').split('\n')[0]; } catch (_) {}
  }
  assert.ok(port, 'Chromium did not start. Install Chromium, or set CHROMIUM to its path.');
  const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json();
  ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener('open', r));
  ws.addEventListener('message', (m) => {
    const msg = JSON.parse(m.data);
    if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
    for (let i = waiters.length - 1; i >= 0; i--) {
      if (waiters[i].method === msg.method) { waiters[i].resolve(msg.params); waiters.splice(i, 1); }
    }
  });
  await send('Page.enable');
  await send('Emulation.setDeviceMetricsOverride', { width: WIDTH, height: 800, deviceScaleFactor: 1, mobile: false });
});

after(async () => {
  ws?.close();
  if (chrome && chrome.exitCode === null) {
    const exited = new Promise((r) => chrome.once('exit', r));
    chrome.kill();
    await exited;
  }
  if (profile) rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
});

test('the correct page gives no finding', async () => {
  await open(pages.correct);
  assert.deepEqual(await run(), { 'edge-cut': [], overflow: [], overlap: [], 'word-break': [] });
});

for (const [name, fault] of [
  ['edge-cut', 'edge-cut'], ['overflow', 'overflow'], ['control-overflow', 'overflow'], ['overlap', 'overlap'], ['word-break', 'word-break'],
]) {
  test(`the ${name} page gives one ${fault} finding and no other finding`, async () => {
    await open(pages[name]);
    const found = await run();
    for (const [helper, list] of Object.entries(found)) {
      assert.equal(list.length, helper === fault ? 1 : 0, `${helper} on the ${name} page: ${JSON.stringify(list)}`);
    }
    assert.equal(found[fault][0].fault, fault);
  });
}

test('a control overflow finding names the control', async () => {
  await open(pages['control-overflow']);
  const [finding] = await evaluate(`(${overflows})()`);
  assert.equal(finding.box, 'control');
  assert.equal(finding.text, 'Releases 20+');
});

test('an open dialog is a layer over the controls under it', async () => {
  await open(pages.overlap.replace('</body>', '<dialog id="layer"><button>Close</button></dialog></body>'));
  await evaluate(`document.getElementById('layer').showModal()`);
  assert.deepEqual(await evaluate(`(${overlaps})()`), []);
});

test('the opt-out attribute removes a break inside a word', async () => {
  await open(page(`<p style="width: 40px; overflow-wrap: anywhere">Releases</p>
    <p ${ALLOW_BREAK_ATTRIBUTE}><span style="display: block; width: 40px; overflow-wrap: anywhere">Releases</span></p>`));
  const found = await evaluate(`(${wordBreaks})()`);
  assert.equal(found.length, 1, JSON.stringify(found));
});

test('the failure message names the record, the width and the fix', () => {
  const message = fitMessage({ fault: 'word-break', text: 'Releases', detail: 'Releases' }, 761);
  assert.match(message, /^musicindex ADR 0065 R-FIT-3: at 761 px the text "Releases" breaks inside a word/);
  assert.match(message, /ADR 0020 R-FIT-1 item 2/);
  assert.match(message, new RegExp(ALLOW_BREAK_ATTRIBUTE));
  assert.match(fitMessage({ fault: 'overflow', box: 'control', text: 'Releases 20+', detail: 'content 94 px in a box of 82 px' }, 761),
    /^musicindex ADR 0065 R-FIT-3: at 761 px the content of the control "Releases 20\+" is wider than the control/);
  for (const fault of ['edge-cut', 'overflow', 'overlap']) {
    assert.match(fitMessage({ fault, text: 'x', detail: '3 px' }, 390), /^musicindex ADR 0065 R-FIT-3: at 390 px .*\n\S/);
  }
});
