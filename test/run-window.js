const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

test('opening, refreshing, and synthetic clicks never execute a pending script', async () => {
  class Element {
    constructor() { this.events = {}; this.scrollTop = 0; this.value = ''; }
    addEventListener(event, callback) { this.events[event] = callback; }
    setAttribute() {}
    append() {}
    replaceChildren() {}
    blur() {}
  }
  const elements = new Map();
  const document = {
    events: {},
    getElementById(id) { if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id); },
    createElement: () => new Element(),
    addEventListener(event, callback) { this.events[event] = callback; },
  };
  let executions = 0;
  let refresh;
  const context = {
    document, console, clearTimeout() {}, setTimeout() { return 1; },
    window: { ramify: {
      runs: async () => [{ id: '20261026-000000-1', status: 'pending', script: 'fixture', cwd: '/fixture' }],
      go: async () => { executions += 1; },
      onRefresh(callback) { refresh = callback; },
    } },
  };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../app/run/renderer.js'), 'utf8'), context);
  await new Promise(setImmediate);
  assert.equal(executions, 0);
  refresh();
  await new Promise(setImmediate);
  assert.equal(executions, 0);
  document.events.keydown({ key: 'Enter', ctrlKey: true });
  elements.get('go').events.click({ isTrusted: false });
  elements.get('go').events.click({ isTrusted: true, detail: 0 });
  await new Promise(setImmediate);
  assert.equal(executions, 0);
  elements.get('go').events.click({ isTrusted: true, detail: 1 });
  await new Promise(setImmediate);
  assert.equal(executions, 1);
});
