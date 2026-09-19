/**
 * One preview header, contextual command ownership, no new edit model.
 * Menus dispatch existing commands; originals, history and the compositor are unchanged.
 */
const FocusUI = (() => {
  let ready = false, observer, dismissTrigger = null;
  const expandedSections = new Map();
  const $f = s => document.querySelector(s);
  const $fa = s => [...document.querySelectorAll(s)];
  const item = (label, icon, run, extra = {}) => ({label, icon, run, ...extra});
  const separator = {separator:true};
  const routeCommand = (action, anchor) => () => route(action, anchor);
  const visible = el => !!el && el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden';

  function showLibrary(tab = 'media') {
    if (ui.exporting || projectBusy) return;
    $('#app').classList.remove('focus-view');
    $('#workspace').classList.remove('library-hidden');
    if (innerWidth <= 860) {$('#app').classList.add('library-drawer');$('#workspace').classList.remove('inspector-open');}
    ui.tab = tab;
    render();
    const next = $('#library input,#library button') || $('[data-tab="media"]');
    next?.focus({preventScroll:true});
    fitCanvas();
  }
  function propertiesVisible() { return visible($('.inspector')); }
  function libraryVisible() { return visible($('.sidebar')); }
  function toggleProperties() {
    const w = $('#workspace'), show = !propertiesVisible();
    if (innerWidth <= 1080) w.classList.toggle('inspector-open',show);
    else w.classList.toggle('properties-hidden',!show);
    if (show && innerWidth<=860) $('#app').classList.remove('library-drawer');
    $('#app').classList.remove('focus-view');
    syncPanelButtons(); fitCanvas();
  }
  function toggleLibrary() {
    const show = !libraryVisible();
    $('#app').classList.remove('focus-view');
    if (innerWidth <= 860) $('#app').classList.toggle('library-drawer',show);
    else $('#workspace').classList.toggle('library-hidden',!show);
    if (show && innerWidth<=860) $('#workspace').classList.remove('inspector-open');
    syncPanelButtons(); fitCanvas();
  }
  function resetView() {
    $('#app').classList.remove('focus-view','library-drawer');
    $('#workspace').classList.remove('library-hidden','properties-hidden','inspector-open');
    syncPanelButtons(); fitCanvas();
  }
  function focusPreview() {
    $('#app').classList.toggle('focus-view');
    $('#app').classList.remove('library-drawer');
    $('#workspace').classList.remove('inspector-open');
    syncPanelButtons(); fitCanvas();
  }
  function syncPanelButtons() {
    const pairs = [['#libraryToggle',libraryVisible()],['#inspectorToggle',propertiesVisible()]];
    for (const [selector, isOpen] of pairs) {
      const el = $(selector); if (!el) continue;
      el.setAttribute('aria-pressed',String(isOpen));
      el.title = `${isOpen?'Hide':'Show'} ${selector==='#libraryToggle'?'media library':'properties'}`;
    }
    $('#reviewButton')?.setAttribute('aria-pressed',String(ui.review));
  }
  function density() {
    const bar = $('.preview-header'); if (!bar) return;
    const width = bar.clientWidth;
    bar.dataset.density = width < 520 ? 'compact' : width < 800 ? 'medium' : 'wide';
    const more=$('#moreToolsButton .tool-label');
    if(more)more.textContent=width<800?'More':'More tools';
    const renderLabel=$('.header-actions .render-label');if(renderLabel)renderLabel.textContent=innerWidth<=620?'Render':'Render video';
    rovingTools(); syncPanelButtons();
  }
  function rovingTools() {
    const buttons = $fa('.toolstrip button').filter(visible);
    if (!buttons.length) return;
    const current = buttons.includes(document.activeElement) ? document.activeElement : buttons.find(b=>b.tabIndex===0) || buttons[0];
    buttons.forEach(b=>b.tabIndex=b===current?0:-1);
  }

  function teachingTarget() {
    const c=selectedClip();
    if(c&&asset(c.asset_id)?.kind==='video')return c;
    return activeClips().filter(c=>asset(c.asset_id)?.kind==='video'&&track(c.track_id)?.visible).sort((a,b)=>project.tracks.indexOf(track(b.track_id))-project.tracks.indexOf(track(a.track_id)))[0]||null;
  }
  function commandItems(kind, anchor) {
    if (kind === 'project') return [
      item('New project…','plus',()=>newEmptyProject()),
      item('Open project…','folder',routeCommand('openProject',anchor)),
      item('Rename tutorial…','edit',routeCommand('rename',anchor)),
      separator,
      item('Workspace & rendered products','layers',()=>showLibrary('project')),
      item('Project files & session…','shield',()=>Quality.showSession()),
      separator,
      item('Restore sample project…','undo',routeCommand('reset',anchor)),
    ];
    if (kind === 'tools') {
      // Controls that overflow at smaller widths reappear here, not in a second row.
      const extra = [];
      for (const [key,label,icon] of [['text','Text','text'],['arrow','Arrow','arrowUpRight'],['highlight','Highlight','box'],['zoom','Zoom','zoomIn']]) {
        if (!visible($(`.toolstrip [data-effect="${key}"]`))) extra.push(item(label,icon,()=>addEffect(key)));
      }
      if(extra.length)extra.push(separator);
      return [...extra,
        item('Spotlight','spotlight',()=>addEffect('spotlight')),
        item('Numbered step','step',()=>addEffect('step')),
        item('Privacy cover','shield',()=>{const c=teachingTarget();if(c){selectClip(c.id);addPrivacyCover();}},{disabled:!teachingTarget()||track(teachingTarget()?.track_id)?.locked,reason:'Select an unlocked video clip, or move the playhead over visible footage.'}),
        separator,
        item('Browse all teaching tools','layers',()=>showLibrary('teach')),
      ];
    }
    if (kind === 'view') return [
      item('Show media library','panelLeft',toggleLibrary,{checked:libraryVisible()}),
      item('Show properties','sliders',toggleProperties,{checked:propertiesVisible()}),
      item('Focus preview','video',focusPreview,{checked:$('#app').classList.contains('focus-view')}),
      item('Reset panel layout','undo',resetView),
      separator,
      item('Light theme','sun',routeCommand('theme',anchor),{checked:document.documentElement.dataset.theme==='light'}),
      separator,
      item('Download annotated frame…','camera',routeCommand('snapshot',anchor)),
      item('Audio mixer…','sliders',routeCommand('mixer',anchor)),
      item('Keyboard shortcuts & help…','info',()=>GuideShowHelp()),
    ];
    return [];
  }
  function GuideShowHelp() { window.VaultBuddyGuide?.showHub(); }
  function openMenu(kind, anchor) {
    if (ui.exporting || projectBusy || document.querySelector('dialog[open]')) return;
    if (menuState?.commandKind===kind) { closeContext(true); return; }
    closeContext(); pause();typeBuffer='';clearTimeout(typeTimer);
    const titles = {project:'Project',tools:'More teaching tools',view:'View & workspace'};
    const subtitles = {project:'Working files, originals and rendered products',tools:'Choose a cue for the current moment',view:'Change your workspace, not your edit'};
    menuState = {target:{type:'commands'},commandKind:kind,anchor,panels:[],title:titles[kind],subtitle:subtitles[kind]};
    $('#menu').hidden=true;
    anchor.setAttribute('aria-expanded','true');
    const r=anchor.getBoundingClientRect();
    openMenuPanel(commandItems(kind,anchor),0,r.left,r.bottom+6);
    const menu=menuState.panels[0].el;
    menu.id='focusCommandMenu'; anchor.setAttribute('aria-controls',menu.id);
  }

  function compactLibrary() {
    const root=$('#library'); if (!root) return;
    // The tabs remain stable while less frequent destinations expose their own heading.
    root.dataset.section=ui.tab;
    if (ui.tab==='media') {
      const importer=root.querySelector('.import'), recorder=root.querySelector('.webcam-entry');
      if (importer && recorder) {
        const actions=document.createElement('div');actions.className='media-actions';
        importer.className='primary import'; importer.innerHTML=svg('upload')+'<b>Import media</b>';
        importer.title='Import videos, audio or still images';importer.setAttribute('aria-label','Import media');
        recorder.innerHTML=svg('webcam')+'<span>Webcam</span>';recorder.title='Record a webcam overlay';recorder.setAttribute('aria-label','Record webcam');
        actions.append(importer,recorder);root.prepend(actions);
      }
      root.querySelectorAll('.library-tip').forEach(n=>n.remove());
      // The instructional copy remains in onboarding, rather than consuming everyday workspace.
      for(const p of root.querySelectorAll(':scope > p')) if(p.textContent.includes('Drag to a track'))p.remove();
      const heading=root.querySelector('.library-heading');if(heading)heading.querySelector('.eyebrow').textContent='Source media';
    }
    if (['project','teach'].includes(ui.tab)) {
      const back=document.createElement('button');back.className='library-back';back.dataset.focusTab='media';back.innerHTML=svg('arrowLeft')+'Back to media';root.prepend(back);
      if(ui.tab==='project') {
        root.querySelectorAll(':scope > [data-action="downloadProject"],:scope > [data-action="save"]').forEach(b=>b.remove());
      }
    }
  }
  function compactInspector() {
    if (!ready) return;
    const root=$('#inspector');
    // Actions have a single persistent home in the timeline; properties stay with the object.
    root.querySelectorAll(':scope > .context-trigger').forEach(n=>n.remove());
    for(const n of root.querySelectorAll(':scope > .preset-row')) if(n.querySelector('[data-editor="locate"]'))n.remove();
    for(const section of root.querySelectorAll(':scope > section.section')) {
      const title=section.querySelector(':scope > h3')?.textContent;
      if(!['Frame & crop','Transform source'].includes(title))continue;
      const key=(ui.selected?.type||'none')+':'+(ui.selected?.id||'')+':'+title;
      const details=document.createElement('details');details.className='section precision-section';details.dataset.precisionKey=key;
      details.open=expandedSections.get(key)===true;
      const summary=document.createElement('summary');summary.textContent=title;details.append(summary);
      const body=document.createElement('div');body.className='precision-body';
      for(const child of [...section.childNodes])if(child.nodeName!=='H3')body.append(child);
      details.append(body);section.replaceWith(details);
    }
  }
  function prepareGuide(name) {
    $('#app').classList.remove('focus-view');
    if (['properties','layout','fades'].includes(name)) {
      $('#workspace').classList.remove('properties-hidden');
      if (innerWidth<=1080)$('#workspace').classList.add('inspector-open');
    }
    if(['media','teach','captions','guide','project'].includes(name)) {
      $('#workspace').classList.remove('library-hidden');
      if(innerWidth<=860)$('#app').classList.add('library-drawer');
    }
    syncPanelButtons();
  }
  function revealPropertyControl(el) {
    const precision=el.closest?.('.precision-section');if(precision)precision.open=true;
  }

  function init() {
    ready=true;
    
    const hideProperties=$('.inspector-heading button');hideProperties.dataset.action='inspector';hideProperties.title='Hide properties';hideProperties.setAttribute('aria-label','Hide properties');
    $('.timeline-toolbar [data-editor="more"]').innerHTML=svg('more')+'<span class="edit-actions-label">Edit actions</span>';
    $('.timeline-toolbar [data-editor="more"]').setAttribute('aria-label','Edit actions');
    $('#previewBadge').title='Preview label only. This label is not included in rendered video.';
    observer=new ResizeObserver(density);observer.observe($('.preview-header'));
    document.addEventListener('toggle',ev=>{if(ev.target.matches?.('.precision-section')&&ev.target.isConnected)expandedSections.set(ev.target.dataset.precisionKey,ev.target.open);},true);
    // Window capture observes the trigger before the shared outside-click handler closes its menu.
    window.addEventListener('pointerdown',ev=>{
      const trigger=ev.target.closest('[data-focus-menu]');
      dismissTrigger=trigger&&menuState?.commandKind===trigger.dataset.focusMenu?trigger:null;
    },true);
    document.addEventListener('click',ev=>{
      const trigger=ev.target.closest('[data-focus-menu],[data-focus-tab]');if(!trigger)return;
      ev.preventDefault();ev.stopImmediatePropagation();
      if(dismissTrigger===trigger){dismissTrigger=null;closeContext(true);return;}
      if(trigger.dataset.focusMenu)openMenu(trigger.dataset.focusMenu,trigger);
      else showLibrary(trigger.dataset.focusTab);
    },true);
    document.addEventListener('keydown',ev=>{
      const trigger=ev.target.closest('[data-focus-menu]');
      if(trigger&&['ArrowDown','ArrowUp'].includes(ev.key)&&!menuState){
        ev.preventDefault();ev.stopImmediatePropagation();openMenu(trigger.dataset.focusMenu,trigger);
        if(ev.key==='ArrowUp')menuState?.panels[0]?.el.querySelector('[data-menu-index]:last-of-type')?.focus();return;
      }
      const toolbar=ev.target.closest('.toolstrip');if(!toolbar||menuState)return;
      const buttons=[...toolbar.querySelectorAll('button')].filter(visible),n=buttons.indexOf(ev.target);
      if(n<0||!['ArrowLeft','ArrowRight','Home','End'].includes(ev.key))return;
      ev.preventDefault();ev.stopImmediatePropagation();
      const next=ev.key==='Home'?0:ev.key==='End'?buttons.length-1:(n+(ev.key==='ArrowRight'?1:-1)+buttons.length)%buttons.length;
      buttons.forEach((b,i)=>b.tabIndex=i===next?0:-1);buttons[next]?.focus();
    },true);
    const priorRoute=route;
    route=function(action,el){
      if(action==='library'){toggleLibrary();return;}
      if(action==='inspector'){toggleProperties();return;}
      priorRoute(action,el);syncPanelButtons();
    };
    // On narrow screens, selecting a property category intentionally reveals the drawer.
    const oldProperty=showProperty;
    showProperty=function(...args){$('#app').classList.remove('focus-view');$('#workspace').classList.remove('properties-hidden');if(innerWidth<=1080)$('#workspace').classList.add('inspector-open');oldProperty(...args);syncPanelButtons();fitCanvas();};
    window.addEventListener('resize',()=>{density();});
    window.VaultBuddyFocus={release:"implementation-reference",showLibrary,openMenu,commandItems:(kind)=>commandItems(kind,$('#projectMenuButton')).filter(i=>!i.separator).map(i=>i.label),resetView,prepareGuide,focusPreview,toggleLibrary,toggleProperties,revealPropertyControl};
    render();density();
  }
  return {init,compactLibrary,compactInspector,syncPanelButtons,prepareGuide};
})();

// Preserve the established lifecycle. UI relocation never commits an edit or a render revision.
const focusLibrary=renderLibrary;
renderLibrary=function(){focusLibrary();FocusUI.compactLibrary();};
const focusInspector=renderInspector;
renderInspector=function(){focusInspector();FocusUI.compactInspector();};
updateSelectionBar=function(){const old=$('#selectionBar');if(old){old.hidden=true;old.replaceChildren();}};
const focusClose=closeContext;
closeContext=function(restore=false){const anchor=menuState?.anchor;if(menuState?.commandKind&&anchor?.isConnected){anchor.setAttribute('aria-expanded','false');anchor.removeAttribute('aria-controls');}focusClose(restore);};
const focusRender=render;
render=function(){focusRender();FocusUI.syncPanelButtons();};

// Additive view fields round-trip with versioned workspace envelopes; older envelopes default safely.
const focusCurrentWorkspace=currentWorkspace;
currentWorkspace=function(){return {...focusCurrentWorkspace(),properties_hidden:$('#workspace').classList.contains('properties-hidden'),properties_open:$('#workspace').classList.contains('inspector-open'),focus_preview:$('#app').classList.contains('focus-view')};};
const focusValidateWorkspace=validateWorkspace;
validateWorkspace=function(envelope){const view=envelope?.workspace||{},extra={properties_hidden:view.properties_hidden===true,properties_open:view.properties_open===true,focus_preview:view.focus_preview===true};const valid=focusValidateWorkspace(envelope);Object.assign(valid.workspace,extra);return valid;};
const focusApplyWorkspace=applyWorkspace;
applyWorkspace=function(view={}){$('#workspace').classList.toggle('properties-hidden',view.properties_hidden===true);$('#workspace').classList.toggle('inspector-open',view.properties_open===true);$('#app').classList.toggle('focus-view',view.focus_preview===true);return focusApplyWorkspace(view);};

// Replace the old always-visible Select button with a pointer-accessible context action.
// Insert before destructive commands so familiar Home/End menu navigation is preserved.
const focusContextItems=contextItems;
contextItems=function(target){
  const items=focusContextItems(target);
  if(ui.selected&&['clip','effect','caption','track','marker'].includes(target.type)){
    let position=items.findIndex(entry=>entry.danger);
    if(position<0)position=items.length;
    items.splice(position,0,menuItem('Clear selection',()=>selectMany([]),{icon:'cursor',key:'V'}));
  }
  return items;
};
