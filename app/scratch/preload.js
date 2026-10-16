const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('ramify', {
  items: () => ipcRenderer.invoke('items'),
  copy: (id) => ipcRenderer.invoke('copy', id),
  done: (id) => ipcRenderer.invoke('done', id),
  close: () => ipcRenderer.invoke('close'),
  onRefresh: (fn) => ipcRenderer.on('refresh', () => fn()),
});
