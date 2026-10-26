// The scratch window: each item is a text an agent handed over with `ramify scratch`, for the
// human to copy. It reads the scratch folder directly: <id>.txt and <id>.title per item. Copy
// writes the text to the clipboard, Done deletes the item. One window per machine: a second
// launch (the next `ramify scratch`) brings the open one to the front and makes it reload.
const { app, BrowserWindow, clipboard, ipcMain } = require('electron');
const launch = require('../launch');
const fs = require('node:fs');
const path = require('node:path');

const ICON = path.join(__dirname, '..', '..', 'assets', 'ramify-512.png');
const DIR = process.env.RAMIFY_SCRATCH_DIR
  || path.join(process.env.RAMIFY_STATE_DIR || '/tmp/ramify', 'scratch');
const ID = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

let win = null;

function file(id, ext) {
  if (!ID.test(id ?? '')) throw new Error(`bad item id ${id}`);
  return path.join(DIR, `${id}.${ext}`);
}

const read = (f) => { try { return fs.readFileSync(f, 'utf8'); } catch { return null; } };

ipcMain.handle('items', () => {
  let names = [];
  try { names = fs.readdirSync(DIR); } catch { return []; }
  return names.filter((n) => n.endsWith('.txt')).sort().map((n) => n.slice(0, -4)).filter((id) => ID.test(id))
    .map((id) => ({ id, title: read(file(id, 'title')) || id, text: read(file(id, 'txt')) ?? '' }));
});

ipcMain.handle('copy', (_e, id) => {
  const text = read(file(id, 'txt'));
  if (text == null) throw new Error('this item is gone');
  clipboard.writeText(text);
});

ipcMain.handle('done', (_e, id) => {
  for (const ext of ['txt', 'title']) fs.rmSync(file(id, ext), { force: true });
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
    width: 720,
    height: 620,
    minWidth: 420,
    minHeight: 320,
    title: 'ramify — scratch',
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
