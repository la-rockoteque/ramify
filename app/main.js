// The app is a window on the CLI: state comes from `ramify dash --json`, every action is a
// `ramify` command. Nothing here knows about slots or services.
const { app, BrowserWindow, ipcMain, shell } = require('electron');
const { execFile, spawn } = require('node:child_process');
const path = require('node:path');
const os = require('node:os');
const fs = require('node:fs/promises');

const RAMIFY = path.join(__dirname, '..', 'bin', 'ramify');

// Launched from the Dock, the app gets launchd's bare PATH: no docker, no lsof, no brew git.
// ponytail: fixed list, read the login shell's PATH if a tool lives somewhere else.
const PATH = ['/opt/homebrew/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin', process.env.PATH]
  .filter(Boolean).join(':');

// Worktree paths the last state reported. Actions only ever target one of these.
let knownRoots = new Set();
// Log files the last state pointed at. The renderer reads only these.
let knownLogs = new Set();
// Ticket pages the last state named: `<jira site>/browse/<KEY>`.
let knownTickets = new Set();
const LOG_TAIL = 256 << 10;

// Each action maps to fixed argv. The renderer picks an action and a root, never a command.
const ACTIONS = {
  up: () => ['up'],
  down: () => ['down'],
  delete: (arg) => ['delete', arg, '--yes'],
  complete: (arg) => ['complete', arg, '--yes'],
  new: (arg) => ['new', arg],
  cleanup: () => ['cleanup'],
  prune: () => ['prune'],
  pruneApply: () => ['prune', '--apply'],
  sharedDown: () => ['shared-down'],
};
const ARG = { delete: /^[A-Za-z0-9._-]+$/, complete: /^[A-Za-z0-9._-]+$/, new: /^[A-Za-z0-9][A-Za-z0-9._-]*$/ };

function env(root) {
  // cwd is $HOME, not a checkout: `delete` and `prune` refuse the tree they run from.
  return { ...process.env, PATH, RAMIFY_ROOT: root };
}

ipcMain.handle('state', () => new Promise((resolve, reject) => {
  execFile(RAMIFY, ['dash', '--json'], { cwd: os.homedir(), env: { ...process.env, PATH }, timeout: 60_000, maxBuffer: 8 << 20 },
    (err, stdout, stderr) => {
      if (err) return reject(new Error(stderr.trim() || err.message));
      let projects;
      try { projects = JSON.parse(stdout); } catch { return reject(new Error(`ramify dash --json: ${stdout.slice(0, 200)}`)); }
      knownRoots = new Set(projects.flatMap((p) => [p.primary, ...p.worktrees.map((w) => w.path)]));
      knownLogs = new Set(projects.flatMap((p) => p.worktrees.flatMap((w) =>
        [...w.errors.map((e) => e.log), ...w.services.map((s) => s.log)])).filter(Boolean));
      knownTickets = new Set(projects.flatMap((p) => (p.jira ? p.worktrees.filter((w) => w.ticket).map((w) => `${p.jira}/browse/${w.ticket}`) : [])));
      resolve(projects);
    });
}));

ipcMain.handle('run', (event, { id, action, root, arg }) => {
  const argv = ACTIONS[action];
  if (!argv) throw new Error(`unknown action ${action}`);
  if (!knownRoots.has(root)) throw new Error(`unknown worktree ${root}`);
  if (ARG[action] && !ARG[action].test(arg ?? '')) throw new Error(`invalid name '${arg}'`);

  return new Promise((resolve) => {
    const child = spawn(RAMIFY, argv(arg), { cwd: os.homedir(), env: env(root), stdio: ['ignore', 'pipe', 'pipe'] });
    const send = (chunk) => event.sender.isDestroyed() || event.sender.send('output', { id, text: chunk.toString() });
    child.stdout.on('data', send);
    child.stderr.on('data', send);
    child.on('error', (e) => { send(`${e.message}\n`); resolve(1); });
    child.on('close', (code) => resolve(code ?? 1));
  });
});

// The tail only: a dev server that ran all day writes more than a dialog should hold.
ipcMain.handle('log', async (_e, file) => {
  if (!knownLogs.has(file)) throw new Error(`unknown log ${file}`);
  const handle = await fs.open(file, 'r');
  try {
    const { size } = await handle.stat();
    const start = Math.max(0, size - LOG_TAIL);
    const { buffer, bytesRead } = await handle.read(Buffer.alloc(size - start), 0, size - start, start);
    const text = buffer.subarray(0, bytesRead).toString('utf8');
    return start > 0 ? `… (first ${start} bytes left out)\n${text.slice(text.indexOf('\n') + 1)}` : text;
  } finally {
    await handle.close();
  }
});

ipcMain.handle('open', (_e, url) => {
  if (!/^http:\/\/localhost:\d+\/?$/.test(url) && !knownTickets.has(url)) throw new Error(`refusing to open ${url}`);
  return shell.openExternal(url);
});

ipcMain.handle('reveal', (_e, dir) => {
  if (!knownRoots.has(dir)) throw new Error(`unknown worktree ${dir}`);
  return shell.openPath(dir);
});

function createWindow() {
  const win = new BrowserWindow({
    width: 1100,
    height: 780,
    minWidth: 640,
    minHeight: 480,
    title: 'ramify',
    icon: path.join(__dirname, '..', 'assets', 'ramify-512.png'),
    webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, sandbox: true },
  });
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  win.webContents.on('will-navigate', (e) => e.preventDefault());
  win.loadFile(path.join(__dirname, 'index.html'));
}

app.whenReady().then(() => {
  if (process.platform === 'darwin') app.dock?.setIcon(path.join(__dirname, '..', 'assets', 'ramify-512.png'));
  createWindow();
  app.on('activate', () => BrowserWindow.getAllWindows().length || createWindow());
});
app.on('window-all-closed', () => process.platform === 'darwin' || app.quit());
