const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('ramify', {
  state: () => ipcRenderer.invoke('state'),
  run: (req) => ipcRenderer.invoke('run', req),
  open: (url) => ipcRenderer.invoke('open', url),
  reveal: (dir) => ipcRenderer.invoke('reveal', dir),
  log: (file) => ipcRenderer.invoke('log', file),
  onOutput: (fn) => ipcRenderer.on('output', (_e, msg) => fn(msg)),
});
