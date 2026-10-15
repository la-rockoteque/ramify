// Lists the open requests, renders the files of the one picked, and turns the two buttons into
// `ramify approve|reject`. Text from the branch goes into the DOM through markdown.js only.
const POLL_MS = 3000;
const $main = document.getElementById('main');
const $queue = document.getElementById('queue');
const $status = document.getElementById('status');
const $decide = document.getElementById('decide');
const $note = document.getElementById('note');
const $approve = document.getElementById('approve');
const $reject = document.getElementById('reject');

let gates = [];
let error = null;
let current = null; // `path\0step`
let file = {};      // key → the file path shown
let busy = false;
let decided = 0;    // decisions made in this window: an empty queue after one closes it
let lastJson = '';

const key = (g) => `${g.path}\0${g.ask.step}`;

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
  const g = gates.find((x) => key(x) === current) ?? gates[0];
  current = g ? key(g) : null;
  $status.textContent = gates.length ? `${gates.length} waiting` : '';

  $queue.replaceChildren(...(gates.length > 1 ? gates.map((x) => el('button', {
    class: `tab${key(x) === current ? ' on' : ''}`, role: 'tab', 'aria-selected': String(key(x) === current),
    onclick: () => { current = key(x); $note.value = ''; render(); },
  }, `${x.worktree} · ${x.ask.step}`)) : []));

  $decide.hidden = !g;
  if (!g) {
    $main.replaceChildren(...[
      error ? el('div', { class: 'error' }, error) : null,
      el('div', { class: 'empty' }, el('strong', {}, 'Nothing waits on you'),
        decided ? 'Every request is answered. This window closes.' : 'A request shows up here when an agent asks for a decision.'),
    ].filter(Boolean));
    return;
  }

  const a = g.ask;
  const shown = a.files.find((f) => f.path === file[current]) ?? a.files[0];
  file[current] = shown?.path;
  const doc = el('article', { class: 'doc' });
  if (shown) doc.append(window.markdown(shown.text));

  $main.replaceChildren(...[
    el('div', { class: 'head' },
      el('h2', {}, `${a.step} — ${a.does ?? 'human gate'}`),
      el('div', { class: 'sub' }, `${g.project} / ${g.worktree} · `, el('code', {}, a.branch), ` · ${a.workflow}/${a.lane} · asked ${a.asked ?? ''}`)),
    error ? el('div', { class: 'error' }, error) : null,
    el('div', { class: 'files' }, a.files.map((f) => el('button', {
      class: `file${f === shown ? ' on' : ''}`, onclick: () => { file[current] = f.path; render(); },
    }, f.path))),
    doc,
  ].filter(Boolean));
  sync();
}

function sync() {
  $approve.disabled = busy || !current;
  $reject.disabled = busy || !current || !$note.value.trim();
}

async function refresh() {
  if (busy) return;
  try {
    const next = await window.ramify.gates();
    error = null;
    const json = JSON.stringify(next);
    if (json === lastJson) return;
    const scroll = $main.scrollTop;
    lastJson = json;
    gates = next;
    render();
    $main.scrollTop = scroll;
    if (!gates.length && decided) setTimeout(() => gates.length || window.ramify.close(), 1500);
  } catch (e) {
    error = e.message;
    render();
  }
}

async function decide(verdict) {
  const g = gates.find((x) => key(x) === current);
  if (!g || busy) return;
  busy = true;
  sync();
  $status.textContent = verdict === 'approve' ? 'approving…' : 'rejecting…';
  try {
    const { code, output } = await window.ramify.decide({ path: g.path, step: g.ask.step, verdict, note: $note.value });
    if (code !== 0) throw new Error(output || `ramify ${verdict} exited ${code}`);
    decided++;
    $note.value = '';
    error = null;
  } catch (e) {
    error = e.message;
  } finally {
    busy = false;
    lastJson = '';
    await refresh();
    render();
  }
}

$approve.addEventListener('click', () => decide('approve'));
$reject.addEventListener('click', () => decide('reject'));
$note.addEventListener('input', sync);
document.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); decide('approve'); }
  if (e.key === 'Escape' && document.activeElement === $note) $note.blur();
});
window.ramify.onRefresh(() => { lastJson = ''; refresh(); });

render();
refresh();
setInterval(refresh, POLL_MS);
