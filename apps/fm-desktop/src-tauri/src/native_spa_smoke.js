(async () => {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (document.querySelector('#app')?.children.length && window.procyonPlugin?.loadToken) {
      break;
    }
    await new Promise((done) => setTimeout(done, 100));
  }
  if (!document.querySelector('#app')?.children.length || !window.procyonPlugin?.loadToken) {
    throw new Error('plugin UI or bridge bootstrap did not load within 10 seconds');
  }
  const invoke = window.__TAURI_INTERNALS__?.invoke;
  if (typeof invoke !== 'function') {
    throw new Error('native invoke transport is unavailable; command denial was not exercised');
  }
  try {
    await invoke('plugin:updater|check');
    throw new Error('plugin child invoked the updater');
  } catch (error) {
    if (!/updater.*not allowed on (window|origin)/i.test(String(error))) {
      throw error;
    }
  }
  window.procyonPlugin.postMessage({
    type: 'save-svg',
    svg: '<svg xmlns="http://www.w3.org/2000/svg" data-native-spa-smoke="acl-denied-and-saved"/>',
  });
})().catch((error) => console.error('native SPA smoke failed:', error));
