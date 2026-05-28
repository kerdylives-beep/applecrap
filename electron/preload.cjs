const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('songRequestApp', {
  getState: () => ipcRenderer.invoke('app:get-state'),
  updateSettings: (settings) => ipcRenderer.invoke('settings:update', settings),
  startBot: () => ipcRenderer.invoke('bot:start'),
  stopBot: () => ipcRenderer.invoke('bot:stop'),
  createManualRequest: (requestedBy, query) => ipcRenderer.invoke('queue:create-manual', { requestedBy, query }),
  removeRequest: (id) => ipcRenderer.invoke('queue:remove', id),
  clearQueue: () => ipcRenderer.invoke('queue:clear'),
  searchAppleMusic: (query) => ipcRenderer.invoke('apple-music:search', query),
  openExternal: (url) => ipcRenderer.invoke('shell:open-external', url),
  copyText: (text) => ipcRenderer.invoke('tools:copy-text', text),
  openTokenGeneratorPrivate: () => ipcRenderer.invoke('tools:open-token-generator-private'),
  onState: (listener) => {
    const wrapped = (_, state) => listener(state)
    ipcRenderer.on('state:update', wrapped)
    return () => ipcRenderer.removeListener('state:update', wrapped)
  },
  onMenuAction: (listener) => {
    const wrapped = (_, action) => listener(action)
    ipcRenderer.on('menu:action', wrapped)
    return () => ipcRenderer.removeListener('menu:action', wrapped)
  },
})
