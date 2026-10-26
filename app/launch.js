// Acknowledge the actual window, including a handoff to an existing instance.
const { app } = require('electron');
const fs = require('node:fs');
const path = require('node:path');

const profile = path.join(app.getPath('appData'), 'ramify', app.getName(), process.env.RAMIFY_APP_BUILD || 'dev');
fs.mkdirSync(profile, { recursive: true });
app.setPath('userData', profile);

function acknowledge(win, file) {
  if (!file || win.isDestroyed() || !win.isVisible() || win.webContents.isLoadingMainFrame()) return;
  try {
    fs.writeFileSync(file, JSON.stringify({ pid: process.pid, title: win.getTitle(), visible: true }), { flag: 'wx' });
  } catch (error) {
    if (error.code !== 'EEXIST' && error.code !== 'ENOENT') console.error(error);
  }
}

app.on('browser-window-created', (_event, win) => {
  const ready = () => acknowledge(win, process.env.RAMIFY_LAUNCH_READY);
  win.webContents.on('did-finish-load', ready);
  win.webContents.on('did-stop-loading', ready);
  win.on('show', ready);
  win.on('ready-to-show', () => setImmediate(ready));
});

function handoff(win, data) {
  const ready = () => acknowledge(win, data?.ramifyReady);
  if (win.webContents.isLoadingMainFrame()) win.webContents.once('did-stop-loading', ready);
  else setImmediate(ready);
}

module.exports = { handoff, data: { ramifyReady: process.env.RAMIFY_LAUNCH_READY } };
