const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('ramify', {
  runs: () => ipcRenderer.invoke('runs'),
  go: (id) => ipcRenderer.invoke('go', id),
  block: (req) => ipcRenderer.invoke('block', req),
  close: () => ipcRenderer.invoke('close'),
  onRefresh: (fn) => ipcRenderer.on('refresh', () => fn()),
});
