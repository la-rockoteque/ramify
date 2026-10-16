// Lists the runs, shows the script of the one picked with its line numbers, and turns the two
// buttons into `ramify run go|block`. Script text and output go into the DOM as text only.
const POLL_MS = 3000;
const POLL_RUNNING_MS = 1000;
const $main = document.getElementById('main');
const $queue = document.getElementById('queue');
const $status = document.getElementById('status');
const $decide = document.getElementById('decide');
const $note = document.getElementById('note');
const $go = document.getElementById('go');
const $block = document.getElementById('block');
const $where = document.getElementById('where');

let runs = [];
let error = null;
let current = null; // run id
let busy = false;
let lastJson = '';
let timer = null;

const open = (r) => r.status === 'pending' || r.status === 'running';

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

function badge(r) {
  if (r.status === 'ran') return el('span', { class: `badge ${r.exit === 0 ? 'ok' : 'bad'}` }, `ran · exit ${r.exit}`);
  if (r.status === 'blocked') return el('span', { class: 'badge bad' }, 'blocked');
  return el('span', { class: `badge ${r.status}` }, r.status);
}

// A shell comment line is dimmed. Nothing else is coloured: the human reads the script as it is.
function code(text) {
  const lines = text.replace(/\n$/, '').split('\n');
  return el('div', { class: 'code' }, el('table', {}, el('tbody', {}, lines.map((l, i) => el('tr', {},
    el('td', { class: 'n' }, String(i + 1)),
    el('td', { class: /^\s*#/.test(l) ? 'c' : '' }, l || ' '))))));
}

function render() {
  const r = runs.find((x) => x.id === current) ?? runs.find(open) ?? runs[0];
  current = r?.id ?? null;
  const waiting = runs.filter((x) => x.status === 'pending').length;
  $status.textContent = waiting ? `${waiting} waiting` : '';

  $queue.replaceChildren(...(runs.length > 1 ? runs.map((x) => el('button', {
    class: `tab${x.id === current ? ' on' : ''}`, role: 'tab', 'aria-selected': String(x.id === current),
    onclick: () => { current = x.id; $note.value = ''; render(); },
  }, `${open(x) ? '▶ ' : ''}${x.worktree ?? x.id}${x.note ? ` · ${x.note.slice(0, 28)}` : ''}`)) : []));

  $decide.hidden = !r || r.status !== 'pending';
  if (!r) {
    $main.replaceChildren(...[
      error ? el('div', { class: 'error' }, error) : null,
      el('div', { class: 'empty' }, el('strong', {}, 'Nothing waits on you'),
        'A script shows up here when an agent asks you to run it.'),
    ].filter(Boolean));
    return;
  }
  $where.textContent = `Runs as you, in ${r.cwd}`;

  const meta = el('dl', { class: 'meta' },
    el('dt', {}, 'directory'), el('dd', {}, r.cwd),
    el('dt', {}, 'source'), el('dd', {}, r.source),
    el('dt', {}, 'sha256'), el('dd', {}, r.sha256),
    el('dt', {}, 'asked'), el('dd', {}, r.created),
    r.started ? [el('dt', {}, 'started'), el('dd', {}, r.started)] : null,
    r.finished ? [el('dt', {}, 'finished'), el('dd', {}, r.finished)] : null,
    r.decided ? [el('dt', {}, 'decided'), el('dd', {}, r.decided)] : null);

  $main.replaceChildren(...[
    el('div', { class: 'head' },
      el('h2', {}, `run ${r.id} `, badge(r)),
      el('div', { class: 'sub' }, [r.project, r.worktree].filter(Boolean).join(' / ') || 'outside a checkout')),
    error ? el('div', { class: 'error' }, error) : null,
    r.note ? el('div', { class: 'note' }, r.note) : null,
    r.status === 'blocked' && r.reason ? el('div', { class: 'error' }, `Blocked: ${r.reason}`) : null,
    meta,
    r.output != null ? [el('div', { class: 'label' }, r.status === 'running' ? 'Output (running…)' : 'Output'),
      el('pre', { class: 'out', id: 'out' }, r.output || '(no output)')] : null,
    el('div', { class: 'label' }, 'Script'),
    code(r.script),
  ].flat().filter(Boolean));
  const out = document.getElementById('out');
  if (out && r.status === 'running') out.scrollTop = out.scrollHeight;
  sync();
}

function sync() {
  const r = runs.find((x) => x.id === current);
  const can = !busy && r?.status === 'pending';
  $go.disabled = !can;
  $block.disabled = !can;
}

function schedule() {
  clearTimeout(timer);
  timer = setTimeout(refresh, runs.some((r) => r.status === 'running') ? POLL_RUNNING_MS : POLL_MS);
}

async function refresh() {
  try {
    if (busy) return;
    const next = await window.ramify.runs();
    error = null;
    const json = JSON.stringify(next);
    if (json === lastJson) return;
    const scroll = $main.scrollTop;
    lastJson = json;
    runs = next;
    render();
    $main.scrollTop = scroll;
  } catch (e) {
    error = e.message;
    render();
  } finally {
    schedule();
  }
}

async function act(verb) {
  const r = runs.find((x) => x.id === current);
  if (!r || r.status !== 'pending' || busy) return;
  busy = true;
  sync();
  $status.textContent = verb === 'go' ? 'starting…' : 'blocking…';
  try {
    if (verb === 'go') {
      await window.ramify.go(r.id);
    } else {
      const { code, output } = await window.ramify.block({ id: r.id, note: $note.value });
      if (code !== 0) throw new Error(output || `ramify run block exited ${code}`);
    }
    $note.value = '';
    error = null;
  } catch (e) {
    error = e.message;
  } finally {
    busy = false;
    lastJson = '';
    await refresh();
  }
}

// No keyboard shortcut for Run: it takes a deliberate click.
$go.addEventListener('click', () => act('go'));
$block.addEventListener('click', () => act('block'));
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && document.activeElement === $note) $note.blur();
});
window.ramify.onRefresh(() => { lastJson = ''; refresh(); });

render();
refresh();
