// The run window: each run is a script an agent handed over with `ramify run ask`, for the human
// to read, then run or block. It reads the runs folder directly: <id>/script, meta.json and
// output.log. Run starts `ramify run go <id>` detached, so a script outlives the window; Block is
// `ramify run block`. One window per machine: a second launch (the next `ramify run ask`) brings
// the open one to the front and makes it reload.
const { app, BrowserWindow, ipcMain } = require('electron');
const launch = require('../launch');
const { execFile, spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const RAMIFY = path.join(__dirname, '..', '..', 'bin', 'ramify');
const ICON = path.join(__dirname, '..', '..', 'assets', 'ramify-512.png');
// Launched from a hook or the Dock, the app gets a bare PATH. Same list as the dashboard.
const PATH = ['/opt/homebrew/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin', process.env.PATH]
  .filter(Boolean).join(':');
const DIR = process.env.RAMIFY_RUNS_DIR
  || path.join(process.env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), '.claude'), 'ramify', 'runs');
const ID = /^[0-9]{8}-[0-9]{6}-[0-9]+$/;
const RECENT = 20;           // finished runs the window keeps showing
const OUTPUT_MAX = 256 << 10; // the tail of output.log the window shows

let win = null;

function dir(id) {
  if (!ID.test(id ?? '')) throw new Error(`bad run id ${id}`);
  return path.join(DIR, id);
}

const read = (f) => { try { return fs.readFileSync(f, 'utf8'); } catch { return null; } };

function tail(f) {
  let fd;
  try {
    fd = fs.openSync(f, 'r');
    const size = fs.fstatSync(fd).size;
    const start = Math.max(0, size - OUTPUT_MAX);
    const buf = Buffer.alloc(size - start);
    fs.readSync(fd, buf, 0, buf.length, start);
    return (start ? '…\n' : '') + buf.toString('utf8');
  } catch {
    return null;
  } finally {
    if (fd !== undefined) fs.closeSync(fd);
  }
}

function ramify(argv) {
  return new Promise((resolve) => {
    execFile(RAMIFY, argv, { cwd: os.homedir(), env: { ...process.env, PATH, RAMIFY_RUNS_DIR: DIR }, timeout: 60_000 },
      (err, stdout, stderr) => resolve({ code: err ? (typeof err.code === 'number' ? err.code : 1) : 0, output: (stdout + stderr).trim() }));
  });
}

// Pending and running first, then the most recent finished ones.
ipcMain.handle('runs', () => {
  let names = [];
  try { names = fs.readdirSync(DIR).filter((n) => ID.test(n)).sort().reverse(); } catch { return []; }
  const all = names.map((id) => {
    let meta;
    try { meta = JSON.parse(read(path.join(DIR, id, 'meta.json'))); } catch { return null; }
    return { ...meta, id, script: read(path.join(DIR, id, 'script')) ?? '', output: tail(path.join(DIR, id, 'output.log')) };
  }).filter(Boolean);
  const open = all.filter((r) => r.status === 'pending' || r.status === 'running');
  const done = all.filter((r) => r.status !== 'pending' && r.status !== 'running').slice(0, RECENT);
  return [...open, ...done];
});

ipcMain.handle('go', (_e, id) => {
  const d = dir(id);
  const meta = JSON.parse(read(path.join(d, 'meta.json')) ?? 'null');
  if (meta?.status !== 'pending') throw new Error(`run ${id} is not pending`);
  // Detached and unreferenced: closing the window must not kill a script halfway through.
  const child = spawn(RAMIFY, ['run', 'go', id], {
    cwd: os.homedir(), env: { ...process.env, PATH, RAMIFY_RUNS_DIR: DIR }, detached: true, stdio: 'ignore',
  });
  child.unref();
});

ipcMain.handle('block', async (_e, { id, note }) => {
  dir(id);
  const text = String(note ?? '').trim();
  return ramify(['run', 'block', id, ...(text ? ['--note', text] : [])]);
});

ipcMain.handle('close', () => win?.close());

function front() {
  if (!win) return;
  if (win.isMinimized()) win.restore();
  win.show();
  win.focus();
  if (process.platform === 'darwin') app.focus({ steal: true });
}

function createWindow() {
  win = new BrowserWindow({
    width: 900,
    height: 860,
    minWidth: 520,
    minHeight: 420,
    title: 'ramify — run',
    icon: ICON,
    show: false,
    webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, sandbox: true },
  });
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  win.webContents.on('will-navigate', (e) => e.preventDefault());
  win.once('ready-to-show', front);
  win.on('closed', () => { win = null; });
  win.loadFile(path.join(__dirname, 'index.html'));
}

if (!app.requestSingleInstanceLock(launch.data)) {
  app.quit();
} else {
  app.on('second-instance', (_event, _argv, _cwd, data) => {
    if (!win) return createWindow();
    win.webContents.send('refresh');
    front();
    launch.handoff(win, data);
  });
  app.whenReady().then(() => {
    if (process.platform === 'darwin') app.dock?.setIcon(ICON);
    createWindow();
  });
  // A popup, not a resident app: closing the window ends it, on macOS too.
  app.on('window-all-closed', () => app.quit());
}
