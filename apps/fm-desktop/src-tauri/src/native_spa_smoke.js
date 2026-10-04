(async () => {
  const smokeToken = '__PROCYON_SMOKE_TOKEN__';
  const stage = async (name, error) => {
    const params = new URLSearchParams({ token: smokeToken, stage: name });
    if (error) params.set('error', String(error).slice(0, 256));
    const response = await fetch(`/smoke?${params}`);
    if (!response.ok) throw new Error(`smoke stage ${name} rejected: ${response.status}`);
  };
  try {
    await stage('script-entered');
    for (let attempt = 0; attempt < 100; attempt++) {
      if (document.querySelector('#app')?.children.length && window.procyonPlugin?.loadToken) {
        break;
      }
      await new Promise((done) => setTimeout(done, 100));
    }
    if (!document.querySelector('#app')?.children.length || !window.procyonPlugin?.loadToken) {
      throw new Error('plugin UI or bridge bootstrap did not load within 10 seconds');
    }
    if (window.procyonPlugin.loadToken !== smokeToken) {
      throw new Error('plugin bridge bootstrap token does not match smoke session');
    }
    await stage('plugin-ui-ready');
    const invoke = window.__TAURI_INTERNALS__?.invoke;
    if (typeof invoke !== 'function') {
      throw new Error('native invoke transport is unavailable; command denial was not exercised');
    }
    try {
      await invoke('plugin:updater|check');
      throw new Error('plugin child invoked the updater');
    } catch (error) {
      const denial = String(error);
      if (
        !/^Command plugin:updater\|check not allowed by ACL$/i.test(denial) &&
        !/updater.*not allowed on (window|origin)/i.test(denial)
      ) {
        throw error;
      }
    }
    await stage('acl-denied');
    await stage('save-requested');
    window.procyonPlugin.postMessage({
      type: 'save-svg',
      svg: '<svg xmlns="http://www.w3.org/2000/svg" data-native-spa-smoke="acl-denied-and-saved"/>',
    });
  } catch (error) {
    console.error('native SPA smoke failed:', error);
    await stage('script-failed', error);
  }
})();
