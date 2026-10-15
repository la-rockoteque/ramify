const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('ramify', {
  gates: () => ipcRenderer.invoke('gates'),
  decide: (req) => ipcRenderer.invoke('decide', req),
  close: () => ipcRenderer.invoke('close'),
  onRefresh: (fn) => ipcRenderer.on('refresh', () => fn()),
});
