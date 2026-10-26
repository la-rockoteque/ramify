const test = require('node:test');
const assert = require('node:assert/strict');
const { EventEmitter } = require('node:events');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function fixture() {
  const app = new EventEmitter();
  app.getPath = () => '/fixture';
  app.getName = () => 'ramify-test';
  app.setPath = () => {};
  const writes = [];
  const context = {
    require: (name) => ({
      electron: { app },
      'node:fs': { mkdirSync() {}, writeFileSync(file, body) { writes.push({ file, body: JSON.parse(body) }); } },
      'node:path': path,
    })[name],
    process: { env: { RAMIFY_LAUNCH_READY: '/ready' }, pid: 42 },
    module: { exports: {} }, console, setImmediate,
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../app/launch.js'), 'utf8'), context);
  const win = new EventEmitter();
  win.webContents = new EventEmitter();
  win.loading = true;
  win.visible = true;
  win.isDestroyed = () => false;
  win.isVisible = () => win.visible;
  win.getTitle = () => 'ramify — test';
  win.webContents.isLoadingMainFrame = () => win.loading;
  app.emit('browser-window-created', {}, win);
  return { win, writes, launch: context.module.exports };
}

test('cold launch waits until navigation stops loading', () => {
  const { win, writes } = fixture();
  win.webContents.emit('did-finish-load');
  assert.equal(writes.length, 0);
  win.loading = false;
  win.webContents.emit('did-stop-loading');
  assert.equal(writes[0].file, '/ready');
  assert.equal(writes[0].body.visible, true);
});

test('a hidden window does not acknowledge readiness until shown', () => {
  const { win, writes } = fixture();
  win.loading = false;
  win.visible = false;
  win.webContents.emit('did-stop-loading');
  assert.equal(writes.length, 0);
  win.visible = true;
  win.emit('show');
  assert.equal(writes.length, 1);
});

test('an existing instance acknowledges the new request after handoff', async () => {
  const { win, writes, launch } = fixture();
  win.loading = false;
  launch.handoff(win, { ramifyReady: '/second-ready' });
  await new Promise(setImmediate);
  assert.equal(writes[0].file, '/second-ready');
});

test('a handoff during navigation waits for loading to stop', () => {
  const { win, writes, launch } = fixture();
  launch.handoff(win, { ramifyReady: '/second-ready' });
  win.webContents.emit('did-finish-load');
  assert.equal(writes.length, 0);
  win.loading = false;
  win.webContents.emit('did-stop-loading');
  assert.ok(writes.some((write) => write.file === '/second-ready'));
});
