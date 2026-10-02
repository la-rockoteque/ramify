// Renders `ramify dash --json` and turns buttons into `ramify` commands. Data goes into the DOM
// through textContent only: branch names and paths are not ours.
const POLL_MS = 3000;
const $projects = document.getElementById('projects');
const $status = document.getElementById('status');
const $log = document.getElementById('log');
const $tabs = document.getElementById('tabs');

const busy = new Map(); // root → action label
let nextId = 0;
let polling = false;

function el(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === 'class') node.className = v;
    else if (k.startsWith('on')) node.addEventListener(k.slice(2), v);
    else if (v !== false && v != null) node.setAttribute(k, v === true ? '' : v);
  }
  for (const c of children.flat()) if (c != null && c !== false) node.append(c);
  return node;
}

function logLine(text, cls) {
  const atBottom = $log.scrollTop + $log.clientHeight >= $log.scrollHeight - 4;
  $log.append(cls ? el('span', { class: cls }, text) : text);
  if (atBottom) $log.scrollTop = $log.scrollHeight;
}

const streams = new Map(); // id → collected output
window.ramify.onOutput(({ id, text }) => {
  streams.set(id, (streams.get(id) ?? '') + text);
  logLine(text);
});

// Runs one action and returns { code, output }. The card stays marked busy until it ends.
async function run(action, root, label, arg) {
  const id = ++nextId;
  busy.set(root, label);
  render();
  logLine(`\n$ ramify ${label}\n`, 'cmd');
  let code;
  try {
    code = await window.ramify.run({ id, action, root, arg });
  } catch (e) {
    logLine(`${e.message}\n`, 'fail');
    code = 1;
  }
  if (code !== 0) logLine(`exit ${code}\n`, 'fail');
  const output = streams.get(id) ?? '';
  streams.delete(id);
  busy.delete(root);
  render();
  refresh();
  return { code, output };
}

let projects = null;
let error = null;
let lastJson = '';
const drafts = new Map(); // primary → slug being typed

// The selected repo, by primary path: two repos can share a directory name. A per-viewer
// convenience, so a failing storage only costs the choice.
const TAB_KEY = 'ramify.tab';
let selected = null;
try { selected = localStorage.getItem(TAB_KEY); } catch { /* private storage off */ }

function select(primary) {
  selected = primary;
  try { localStorage.setItem(TAB_KEY, primary); } catch { /* keep it in memory only */ }
  render();
}

function tabBar() {
  return projects.map((p, i) => {
    const running = p.worktrees.filter((w) => w.slot != null).length;
    const working = [...busy.keys()].some((root) => root === p.primary || p.worktrees.some((w) => w.path === root));
    const on = p.primary === selected;
    return el('button', {
      class: `tab${on ? ' on' : ''}`, role: 'tab', 'aria-selected': on ? 'true' : 'false',
      title: `${p.primary}${i < 9 ? `  (⌘${i + 1})` : ''}`, onclick: () => select(p.primary),
    }, p.name,
    p.stacks ? el('span', { class: `count${running ? ' live' : ''}`, 'aria-label': `${running} running` }, String(running)) : null,
    working ? el('span', { class: 'dot starting', 'aria-label': 'command running' }) : null);
  });
}

async function refresh() {
  if (polling) return;
  polling = true;
  try {
    const next = await window.ramify.state();
    $status.textContent = `updated ${new Date().toLocaleTimeString()}`;
    const json = JSON.stringify(next);
    // Unchanged state, no redraw: a rebuild under the pointer eats clicks and hovers.
    if (json === lastJson && !error) return;
    projects = next; lastJson = json; error = null;
  } catch (e) {
    error = e.message;
    $status.textContent = `refresh failed: ${e.message}`;
  } finally {
    polling = false;
  }
  render();
}

// ── Logs ──
const $logview = document.getElementById('logview');
let logFile = null;

async function loadLog() {
  const $body = $logview.querySelector('pre');
  try {
    $body.textContent = (await window.ramify.log(logFile)) || '(empty log)';
  } catch (e) {
    $body.textContent = e.message;
  }
  $body.scrollTop = $body.scrollHeight;
}

function showLog(title, message, file) {
  logFile = file;
  $logview.querySelector('h3').textContent = title;
  $logview.querySelector('.message').textContent = message;
  $logview.querySelector('.path').textContent = file ?? '';
  $logview.querySelector('pre').textContent = file ? 'loading…' : 'No log for this step: the message above is all ramify printed.';
  $logview.querySelector('.reload').hidden = !file;
  $logview.showModal();
  if (file) loadLog();
}
$logview.querySelector('.reload').addEventListener('click', loadLog);
$logview.querySelector('.close').addEventListener('click', () => $logview.close());

// A "!" that opens the log. Its label says what failed, for the pointer and for screen readers.
function bang(title, message, file) {
  return el('button', { class: 'bang', title: `${message} (click for the log)`, 'aria-label': `${title}: ${message}. Show the log`,
    onclick: () => showLog(title, message, file) }, '!');
}

// Failed: `up` recorded why, or a service it started no longer answers at all.
function serviceRow(w, s) {
  const url = `http://localhost:${s.port}`;
  const error = w.errors.find((e) => e.service === s.name);
  const crashed = s.state === 'down';
  const failed = error || crashed;
  return el('div', { class: 'svc' },
    failed
      ? bang(`${w.name} · ${s.name}`, error?.message ?? `nothing answers on :${s.port} any more`, error?.log ?? s.log)
      : el('span', { class: `dot ${s.state}`, title: s.state }),
    el('span', {},
      el('a', { href: '#', onclick: (e) => { e.preventDefault(); window.ramify.open(url); } }, url),
      ' ', el('span', { class: 'kind' }, s.kind === 'own' ? s.name : `${s.name} · ${s.kind}`)),
    el('span', { class: 'kind' }, s.state));
}

function worktreeCard(project, w) {
  const running = w.slot != null;
  const doing = busy.get(w.path);
  const branch = el('div', { class: 'card-branch' }, w.branch, w.dirty ? el('span', { class: 'dirty', title: 'uncommitted changes' }, ' ●') : null);
  const ticketUrl = project.jira && w.ticket ? `${project.jira}/browse/${w.ticket}` : null;
  const ticket = ticketUrl
    ? el('a', { class: 'ticket', href: '#', title: `Open ${w.ticket} in Jira`, onclick: (e) => { e.preventDefault(); window.ramify.open(ticketUrl); } }, w.ticket)
    : null;
  const meta = el('div', { class: 'meta' },
    doing ? `${doing}…` : running ? `slot ${w.slot} · up ${w.uptime}` : project.stacks ? 'stopped' : '',
    w.primary ? el('div', {}, 'primary') : null);

  const actions = el('div', { class: 'actions' },
    !project.stacks ? null : running
      ? el('button', { disabled: !!doing, onclick: () => run('down', w.path, `down  (${w.name})`) }, 'Stop')
      : el('button', { class: 'primary', disabled: !!doing, onclick: () => run('up', w.path, `up  (${w.name})`) }, 'Start QA'),
    el('button', { onclick: () => window.ramify.reveal(w.path) }, 'Open folder'),
    w.primary ? null : el('button', {
      disabled: !!doing,
      title: 'Close the ticket, stop the stack, remove the worktree and its local branch',
      onclick: () => {
        const what = w.ticket ? `close ${w.ticket}, then remove` : 'remove';
        if (confirm(`Complete ${w.name}: ${what} the worktree and the local branch ${w.branch}?\n\nThe remote branch is kept. Uncommitted or unpushed work makes it refuse.`)) {
          run('complete', project.primary, `complete ${w.name} --yes`, w.name);
        }
      },
    }, 'Complete'),
    w.primary ? null : el('button', {
      class: 'danger', disabled: !!doing,
      onclick: () => {
        const warn = w.dirty ? '\n\nIt has uncommitted changes.' : '';
        if (confirm(`Delete worktree ${w.name} and branch ${w.branch}? This cannot be undone.${warn}`)) {
          run('delete', project.primary, `delete ${w.name} --yes`, w.name);
        }
      },
    }, 'Delete'));

  return el('div', { class: `card${running || !project.stacks ? '' : ' idle'}${doing ? ' busy' : ''}` },
    el('div', { class: 'card-top' }, el('div', {}, el('div', { class: 'card-name' }, w.name, ticket), branch), meta),
    // Failures of a step, not of a listed service: containers, slots, bootstrap, or a service
    // whose failed prepare took the whole stack down with it.
    w.errors.filter((e) => !w.services.some((s) => s.name === e.service)).map((e) =>
      el('div', { class: 'svc failed-step' }, bang(`${w.name} · ${e.service}`, e.message, e.log),
        el('span', {}, `${e.service}: ${e.message}`))),
    w.services.map((s) => serviceRow(w, s)),
    actions);
}

async function prune(project) {
  const { code, output } = await run('prune', project.primary, `prune  (${project.name})`);
  if (code !== 0 || !/ to prune/.test(output)) return;
  // The branch lines, and the ticket line indented under each.
  const list = output.split('\n').filter((l) => /^  prune |^ {9}\S/.test(l)).map((l) => l.trim()).join('\n');
  if (confirm(`Prune these branches and their worktrees?\n\n${list}`)) {
    run('pruneApply', project.primary, `prune --apply  (${project.name})`);
  }
}

// Running stacks first, the primary checkout first among equals; git's order otherwise.
const byRunning = (list) => [...list].sort((a, b) => (b.slot != null) - (a.slot != null) || b.primary - a.primary);

function projectSection(p) {
  const pdoing = busy.get(p.primary);
  const input = el('input', {
    placeholder: 'new branch slug', 'aria-label': 'New branch slug', 'data-primary': p.primary,
    oninput: () => drafts.set(p.primary, input.value),
  });
  input.value = drafts.get(p.primary) ?? '';
  const create = () => {
    const slug = input.value.trim();
    if (slug) run('new', p.primary, `new ${slug}`, slug);
    drafts.delete(p.primary);
    input.value = '';
  };
  input.addEventListener('keydown', (e) => e.key === 'Enter' && create());

  return el('section', { class: 'project' },
    el('div', { class: 'project-head' },
      el('h2', { title: p.primary }, p.primary.replace(/^\/Users\/[^/]+/, '~')),
      p.containers.map((c) => el('span', { class: 'pill', title: 'shared container' }, el('span', { class: `dot ${c.up}` }), c.name)),
      p.shared.map((s) => el('span', { class: 'pill', title: 'shared instance' }, el('span', { class: `dot ${s.up}` }), `shared ${s.service} :${s.port}`)),
      (p.build_servers ?? []).map((b) => el('span', {
        class: 'pill', title: 'build server, shared by every checkout on the machine — `dotnet build-server shutdown` stops it',
      }, `${b.kind} ×${b.count} ${b.mb >= 1024 ? `${(b.mb / 1024).toFixed(1)} GB` : `${b.mb} MB`}`)),
      el('div', { class: 'project-actions' },
        input,
        el('button', { disabled: !!pdoing, onclick: create }, 'New worktree'),
        el('button', { disabled: !!pdoing, onclick: () => prune(p) }, 'Prune merged'),
        p.stacks ? el('button', { disabled: !!pdoing, onclick: () => run('cleanup', p.primary, `cleanup  (${p.name})`) }, 'Cleanup') : null,
        p.shared.length ? el('button', {
          class: 'danger', disabled: !!pdoing,
          onclick: () => confirm('Stop the shared instances? Every worktree that uses them loses them.')
            && run('sharedDown', p.primary, `shared-down  (${p.name})`),
        }, 'Stop shared') : null)),
    p.stacks ? null : el('div', { class: 'note' }, 'Branching only: this config declares no services, so there is no QA stack to run.'),
    p.orphans.map((o) => el('div', { class: 'orphan' },
      `slot ${o.slot}: orphan of ${o.worktree} (its state moved to slot ${o.moved_to}). Cleanup stops it.`)),
    (p.strays ?? []).map((x) => el('div', { class: 'orphan' },
      `stray: ${x.worktree} ${x.service} still runs (process group ${x.pgid}) with no pid file pointing at it. Cleanup stops it.`)),
    el('div', { class: 'grid' }, byRunning(p.worktrees).map((w) => worktreeCard(p, w))));
}

function render() {
  // Drafts survive the rebuild through `drafts`; focus has to be put back by hand.
  const focused = document.activeElement?.dataset?.primary;

  if (error && !projects) {
    $projects.replaceChildren(el('div', { class: 'empty error' }, error));
  } else if (!projects) {
    $projects.replaceChildren(el('div', { class: 'empty' }, 'loading…'));
  } else if (projects.length === 0) {
    $projects.replaceChildren(el('div', { class: 'empty' }, "No ramify project on this machine yet. Run 'ramify setup' in a repo (or /ramify:setup in Claude Code)."));
  } else {
    // A repo that left the list (its config removed) hands the view to the first one.
    if (!projects.some((p) => p.primary === selected)) selected = projects[0].primary;
    $tabs.replaceChildren(...tabBar());
    $projects.replaceChildren(projectSection(projects.find((p) => p.primary === selected)));
  }
  $tabs.hidden = !projects || projects.length === 0;

  if (focused) {
    const input = [...$projects.querySelectorAll('input')].find((i) => i.dataset.primary === focused);
    input?.focus();
  }
}

document.getElementById('refresh').addEventListener('click', refresh);
document.addEventListener('keydown', (e) => {
  const n = Number(e.key);
  if ((e.metaKey || e.ctrlKey) && n >= 1 && n <= 9 && projects?.[n - 1]) {
    e.preventDefault();
    select(projects[n - 1].primary);
  }
});
refresh();
setInterval(refresh, POLL_MS);
