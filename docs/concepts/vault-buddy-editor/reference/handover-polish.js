/** Final application-shell polish. Does not mutate compositions or source media. */
const HandoverUI = (() => {
  function updateDocumentTitle() {
    const name = document.querySelector('#projectTitle')?.textContent?.trim();
    document.title = name ? `${name} — Vault Buddy Tutorial Editor` : 'Vault Buddy Tutorial Editor';
  }
  function init() {
    updateDocumentTitle();
    const title = document.querySelector('#projectTitle');
    if (title) new MutationObserver(updateDocumentTitle).observe(title, {childList:true, characterData:true, subtree:true});
    document.querySelector('#app')?.setAttribute('aria-busy', 'false');
    const identity = document.querySelector('.brand');
    if (identity) identity.title = 'Vault Buddy · Tutorial Editor';
  }
  function failed(error) {
    console.error('Editor initialization failed', error);
    document.querySelector('#app')?.setAttribute('aria-busy', 'false');
    const panel = document.createElement('section');
    panel.className = 'startup-failure';
    panel.setAttribute('role', 'alert');
    panel.setAttribute('aria-labelledby', 'startupFailureTitle');
    // Use static copy, never interpolate exception messages or imported metadata as HTML.
    panel.innerHTML = '<h1 id="startupFailureTitle" tabindex="-1">The editor could not start</h1><p>Your saved project files have not been changed. Reload this page in a supported desktop browser. Keep your original media and portable project file.</p><button type="button" class="primary">Reload editor</button>';
    document.body.append(panel);
    panel.querySelector('button').addEventListener('click', () => location.reload());
    panel.querySelector('h1').focus();
  }
  return {init,failed};
})();
