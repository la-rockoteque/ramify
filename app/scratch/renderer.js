// Lists the scratch items, shows the text of the one picked as it will paste, and turns the
// buttons into a copy to the clipboard and a delete. The text goes into the DOM as textContent.
const POLL_MS = 2000;
const $main = document.getElementById('main');
const $queue = document.getElementById('queue');
const $status = document.getElementById('status');
const $act = document.getElementById('act');
const $copy = document.getElementById('copy');
const $done = document.getElementById('done');

let items = [];
let error = null;
let current = null; // item id
let cleared = 0;    // items marked done in this window: an empty list after one closes it
let lastJson = '';
let flash = null;

function el(tag, props = {}, ...kids) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === 'class') n.className = v;
    else if (k.startsWith('on')) n.addEventListener(k.slice(2), v);
    else if (v !== false && v != null) n.setAttribute(k, v === true ? '' : v);
  }
  for (const c of kids.flat()) if (c != null && c !== false) n.append(c);
  return n;
}

function render() {
  const it = items.find((x) => x.id === current) ?? items[items.length - 1];
  current = it?.id ?? null;
  $status.textContent = items.length > 1 ? `${items.length} items` : '';

  $queue.replaceChildren(...(items.length > 1 ? items.map((x) => el('button', {
    class: `tab${x.id === current ? ' on' : ''}`, role: 'tab', 'aria-selected': String(x.id === current),
    onclick: () => { current = x.id; render(); },
  }, x.title)) : []));

  $act.hidden = !it;
  if (!it) {
    $main.replaceChildren(...[
      error ? el('div', { class: 'error' }, error) : null,
      el('div', { class: 'empty' }, el('strong', {}, 'Nothing to copy'),
        cleared ? 'Every item is done. This window closes.' : 'An item shows up here when an agent runs ramify scratch.'),
    ].filter(Boolean));
    return;
  }

  const lines = it.text.split('\n').length;
  $main.replaceChildren(...[
    el('div', { class: 'head' },
      el('h2', {}, it.title),
      el('div', { class: 'sub' }, `${it.text.length} characters · ${lines} line${lines === 1 ? '' : 's'}`)),
    error ? el('div', { class: 'error' }, error) : null,
    el('pre', { class: 'text', tabindex: '0' }, it.text),
  ].filter(Boolean));
}

async function refresh() {
  try {
    const next = await window.ramify.items();
    error = null;
    const json = JSON.stringify(next);
    if (json === lastJson) return;
    const scroll = $main.scrollTop;
    const added = next.length > items.length && lastJson;
    lastJson = json;
    items = next;
    if (added) current = items[items.length - 1].id; // a new item comes to the front
    render();
    if (!added) $main.scrollTop = scroll;
    if (!items.length && cleared) setTimeout(() => items.length || window.ramify.close(), 1500);
  } catch (e) {
    error = e.message;
    render();
  }
}

async function copy() {
  if (!current) return;
  try {
    await window.ramify.copy(current);
    error = null;
    $copy.textContent = 'Copied';
    clearTimeout(flash);
    flash = setTimeout(() => { $copy.textContent = 'Copy'; }, 1500);
  } catch (e) {
    error = e.message;
    render();
  }
}

async function done() {
  if (!current) return;
  try {
    await window.ramify.done(current);
    cleared++;
    current = null;
    error = null;
  } catch (e) {
    error = e.message;
  }
  lastJson = '';
  await refresh();
  render();
}

$copy.addEventListener('click', copy);
$done.addEventListener('click', done);
document.addEventListener('keydown', (e) => {
  // ⌘C copies the whole item, unless the human selected part of it.
  if (e.key === 'c' && (e.metaKey || e.ctrlKey) && !String(window.getSelection())) { e.preventDefault(); copy(); }
  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); copy().then(done); }
});
window.ramify.onRefresh(() => { lastJson = ''; refresh(); });

render();
refresh();
setInterval(refresh, POLL_MS);
