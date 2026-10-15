// The approvals window: what waits on a human decision comes from `ramify gates --json`, and each
// decision is `ramify approve|reject`. One window per machine: a second launch (the next
// `ramify step ask`) brings the open one to the front and makes it reload.
const { app, BrowserWindow, ipcMain } = require('electron');
const { execFile } = require('node:child_process');
const path = require('node:path');
const os = require('node:os');

const RAMIFY = path.join(__dirname, '..', '..', 'bin', 'ramify');
const ICON = path.join(__dirname, '..', '..', 'assets', 'ramify-512.png');
// Launched from a hook or the Dock, the app gets a bare PATH. Same list as the dashboard.
const PATH = ['/opt/homebrew/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin', process.env.PATH]
  .filter(Boolean).join(':');
const STEP = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

// `path\0step` of every request the last listing showed. A decision only ever targets one of these.
let known = new Set();
let win = null;

function ramify(argv, env = {}) {
  return new Promise((resolve) => {
    execFile(RAMIFY, argv, { cwd: os.homedir(), env: { ...process.env, PATH, ...env }, timeout: 60_000, maxBuffer: 16 << 20 },
      (err, stdout, stderr) => resolve({ code: err ? (typeof err.code === 'number' ? err.code : 1) : 0, stdout, stderr }));
  });
}

ipcMain.handle('gates', async () => {
  const { code, stdout, stderr } = await ramify(['gates', '--json']);
  if (code !== 0) throw new Error(stderr.trim() || `ramify gates exited ${code}`);
  let gates;
  try { gates = JSON.parse(stdout); } catch { throw new Error(`ramify gates --json: ${stdout.slice(0, 200)}`); }
  known = new Set(gates.map((g) => `${g.path}\0${g.ask.step}`));
  return gates;
});

ipcMain.handle('decide', async (_e, { path: root, step, verdict, note }) => {
  if (verdict !== 'approve' && verdict !== 'reject') throw new Error(`unknown verdict ${verdict}`);
  if (!STEP.test(step ?? '') || !known.has(`${root}\0${step}`)) throw new Error(`no open request for ${step} in ${root}`);
  const text = String(note ?? '').trim();
  if (verdict === 'reject' && !text) throw new Error('a rejection needs a note');
  const argv = [verdict, step, ...(text ? ['--note', text] : [])];
  const { code, stdout, stderr } = await ramify(argv, { RAMIFY_ROOT: root });
  return { code, output: (stdout + stderr).trim() };
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
    width: 860,
    height: 860,
    minWidth: 520,
    minHeight: 420,
    title: 'ramify — approvals',
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

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => {
    if (!win) return createWindow();
    win.webContents.send('refresh');
    front();
  });
  app.whenReady().then(() => {
    if (process.platform === 'darwin') app.dock?.setIcon(ICON);
    createWindow();
  });
  // A popup, not a resident app: closing the window ends it, on macOS too.
  app.on('window-all-closed', () => app.quit());
}
