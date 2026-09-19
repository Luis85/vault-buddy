/* Session safety and validation. No network, telemetry or new media service.
 * UI-only state stays outside editable projects and immutable rendered products.
 */
const Quality = (() => {
  let saveJob = null, lastSave = null, importJob = null, importReport = [];
  let issueKey = null, issueCache = [], issueRuns = 0, issueHits = 0;
  let peak = 0;
  const filmValues = new Map();
  const focusAttributes = ['data-clip','data-fx','data-toggle-track','data-select-track','data-transition','data-seek','data-action'];
  const dialogFocus = new WeakMap();

  function rulerStep(scale) {
    const steps = [.1,.2,.5,1,2,5,10,15,30,60,120,300,600,900,1800,3600,7200,14400];
    return steps.find(n => n * scale >= 70) || 14400;
  }
  function focusToken(element) {
    if (!element || element === document.body) return null;
    if (element.id) return {selector: '#' + CSS.escape(element.id)};
    for (const attr of focusAttributes) if (element.hasAttribute(attr)) {
      let selector = `[${attr}="${CSS.escape(element.getAttribute(attr))}"]`;
      if (element.dataset.key) selector += `[data-key="${CSS.escape(element.dataset.key)}"]`;
      return {selector};
    }
    return null;
  }
  function restoreFocus(token, previous, fallback = null) {
    if (!token || previous?.isConnected) return;
    const target = document.querySelector(token.selector) || fallback;
    if (target && !target.disabled && target.getClientRects().length) target.focus({preventScroll:true});
  }
  function syncFilms() {
    const host = $('#timelineContent'); if (!host) return;
    const active = new Set();
    for (const a of project.assets) if (a.kind === 'video') {
      active.add(a.id);
      const image = thumbnail(a);
      if (filmValues.get(a.id) !== image) {
        host.style.setProperty('--film-' + a.id, image ? `url("${image}")` : 'none');
        filmValues.set(a.id, image);
      }
    }
    for (const id of filmValues.keys()) if (!active.has(id)) {
      host.style.removeProperty('--film-' + id); filmValues.delete(id);
    }
  }
  function releaseSessionMedia() {
    clearRuntime();
    for (const m of new Set(media.values())) if (m.url) URL.revokeObjectURL(m.url);
    media.clear(); thumbs.clear(); productFiles.clear();
    clipClipboard = null; lookClipboard = null; multiIds = [];
    lastPackage = null; lastRenderContext = null; pendingRange = null;
    for(const id of filmValues.keys())$('#timelineContent')?.style.removeProperty('--film-'+id);
    filmValues.clear(); issueKey = null; peak = 0; lastSave = null;
    importReport = [];
  }
  function hasUnsavedTake() {
    return ['countdown','recording','stopping'].includes(webcamState.phase) || !!webcamState.take;
  }
  function beginProjectSave(format) { saveJob = {format,cancelled:false}; return saveJob; }
  function cancelProjectSave() {
    if (!saveJob) return false;
    saveJob.cancelled = true;
    const label = $('#projectSaveFooter .save-preparing');
    if (label) label.textContent = 'Stopping after the current packaging step…';
    const button = $('[data-quality="cancel-save"]'); if (button) button.disabled = true;
    return true;
  }
  function endProjectSave(job) { if (saveJob === job) saveJob = null; }
  function recordSaveRequest(format, name) { lastSave = {format,name,at:nowISO(),projectId:project.id}; }
  function inspectSession() {
    const envelope = workspaceEnvelope(), sources = workspaceAssets(envelope).filter(a => !a.builtin && !a.linked_asset);
    const loaded = sources.filter(a => media.has(a.id));
    const unique = new Set(loaded.map(a => media.get(a.id).file));
    return {
      dirty:documentToken() !== projectDownloadToken,
      recovery:storageWarning ? 'unavailable' : 'best-effort',
      sourceCount:sources.length, loadedCount:loaded.length, missingCount:sources.length-loaded.length,
      sourceBytes:[...unique].reduce((sum,f) => sum + (f?.size || 0),0),
      productCount:envelope.record.products.length,
      loadedProducts:envelope.record.products.filter(p => productFiles.has(p.id)).length,
      mediaEntries:media.size, runtimeCount:runtimes.size,
      lastSave:lastSave?.projectId === project.id ? {...lastSave} : null,
      peak:peak,
      guideStorage:window.VaultBuddyGuide?.getState().storage || 'separate'
    };
  }
  function showSession() {
    if (ui.exporting || projectBusy) return;
    const s = inspectSession();
    $('#sessionBody').innerHTML = `
      <div class="session-lead"><span class="session-icon">${svg('shield')}</span><div><h3>${s.dirty?'Keep an editable copy before closing':'This edit matches a project file'}</h3><p>${s.dirty?'Rendering a video does not save the editable workspace.':'A download request is not confirmation that the file reached your disk.'}</p></div></div>
      <div class="session-facts">
        <article><span>Working project</span><strong>Revision ${ensureRecord().revision}</strong><small>${s.dirty?'Changes need a project download':'No new edit changes since open / download'}</small></article>
        <article><span>Original media</span><strong>${s.loadedCount} / ${s.sourceCount} loaded</strong><small>${formatBytes(s.sourceBytes)} · includes retained render revisions</small></article>
        <article><span>Rendered products</span><strong>${s.productCount}</strong><small>${s.loadedProducts} video files loaded · edits stay separate</small></article>
        <article><span>Browser recovery</span><strong>${s.recovery==='unavailable'?'Session only':'Best-effort cache'}</strong><small>Not a durable backup or a vault write</small></article>
      </div>
      ${s.missingCount?`<div class="callout warning"><b>${s.missingCount} originals are not loaded.</b><br>Saving still preserves your edits. A portable package can include only available files.</div>`:''}
      <section class="session-section"><h3>What each file preserves</h3><p><b>Portable project ZIP</b> — editable tracks, available originals and retained render snapshots. It can contain uncensored source footage.</p><p><b>Lightweight JSON</b> — the edit and workspace. Keep the original files to reconnect.</p><p><b>Rendered video</b> — a viewable product, not an editable source project.</p></section>
      ${s.lastSave?`<p class="field-help">Last request in this session: ${esc(s.lastSave.format==='portable'?'portable project':'lightweight project')} · ${esc(s.lastSave.name)}. Verify it in your download location.</p>`:''}
      <section class="session-section"><h3>Before sharing</h3><p>Watch the encoded video for sound, captions and private details. Guide overlays never enter the composition. No source media is sent to a server by this editor.</p></section>`;
    $('#sessionReconnect').disabled = !s.missingCount;
    openDialog('#sessionDialog');
  }
  function renderImportSummary() {
    $('#library .import-summary')?.remove();
    if (ui.tab !== 'media' || (!importJob && !importReport.length)) return;
    const div = document.createElement('section'); div.className = 'import-summary'; div.setAttribute('aria-live','polite');
    const added = importReport.filter(x=>x.status==='added').length;
    div.innerHTML = importJob
      ? `<b>Importing ${importJob.index} of ${importJob.total}</b><span>Already added: ${added}. Originals are unchanged.</span><button class="btn" data-quality="cancel-import" ${importJob.cancelled?'disabled':''}>${importJob.cancelled?'Stopping…':'Stop after current file'}</button>`
      : `<b>Import finished · ${added} added</b><span>${importReport.length-added} skipped or cancelled.</span><div class="row"><button class="btn" data-quality="import-report">View report</button><button class="icon-btn" data-quality="dismiss-import" aria-label="Dismiss import summary">${svg('x')}</button></div>`;
    $('#library').prepend(div);
  }
  function showImportReport() {
    $('#importReportBody').innerHTML = '<p class="field-help">Successful files remain in Media. Correct a skipped file or choose a supported original and import it again.</p>' + importReport.map(r => `<article class="import-result ${r.status}"><span class="pill">${r.status==='added'?'ADDED':r.status==='cancelled'?'STOPPED':'SKIPPED'}</span><div><b>${esc(r.name)}</b><p>${esc(r.message)}</p></div></article>`).join('');
    openDialog('#importReportDialog');
  }
  function observePeak(value) {
    peak = Math.max(peak,value);
    const meter = $('#peakMeter');
    if (meter) {
      meter.classList.toggle('peak-hot',value >= .98);
      meter.setAttribute('aria-label',`Observed audio sample peak ${value>0?(20*Math.log10(value)).toFixed(1):'-∞'} dBFS`);
    }
  }
  function updateSplitState() {
    const b=$('#splitButton'),c=selectedClip(); if (!b) return;
    let reason='Split the selected clip at the playhead (S)';
    if(!c)reason='Select a clip to split';
    else if(track(c.track_id)?.locked)reason='Unlock this track before splitting';
    else if(transitionsFor(c.id).length)reason='Remove this clip’s transition before splitting';
    else if(ui.time-c.start_ms<MIN || end(c)-ui.time<MIN)reason='Move the playhead inside this clip, at least 0.15 seconds from its edges';
    b.disabled=reason!=='Split the selected clip at the playhead (S)' || ui.exporting || projectBusy;
    b.title=reason;b.setAttribute('aria-label',reason);
  }
  function init() {
    
    const status=$('#saveStatus');status.setAttribute('role','button');status.tabIndex=0;status.dataset.quality='session';status.setAttribute('aria-label','Project files and session status');
    const footer=$('#storageInfo');footer.setAttribute('role','button');footer.tabIndex=0;footer.dataset.quality='session';footer.title='Open project files and recovery details';
    document.body.insertAdjacentHTML('beforeend',`
      <dialog class="dialog session-dialog" id="sessionDialog" aria-labelledby="sessionTitle"><div class="dialog-header"><div><h2 id="sessionTitle">Project files & session</h2><p class="field-help">Your editable source. Its rendered products.</p></div><button class="icon-btn" data-action="close" aria-label="Close project status">${svg('x')}</button></div><div class="dialog-body" id="sessionBody"></div><div class="dialog-footer"><button class="btn" data-quality="session-reconnect" id="sessionReconnect">Reconnect originals</button><button class="btn" data-action="close">Back to edit</button><button class="primary" data-quality="session-save">${svg('save')}Save project</button></div></dialog>
      <dialog class="dialog" id="importReportDialog" aria-labelledby="importReportTitle"><div class="dialog-header"><h2 id="importReportTitle">Media import report</h2><button class="icon-btn" data-action="close" aria-label="Close import report">${svg('x')}</button></div><div class="dialog-body" id="importReportBody"></div><div class="dialog-footer"><button class="primary" data-action="close">Continue editing</button></div></dialog>`);
    // CSS adjusts the short-window layout without persisting a new project revision.
    if(innerHeight<=760 && !document.documentElement.style.getPropertyValue('--timeline')) setTimelineHeight(Math.max(210,Math.round(innerHeight*.36)));
    renderTimeline();updateSplitState();fitCanvas();
    Object.assign(window.VaultBuddyEditor,{collectIssues,importFiles,validate:p=>validate(deep(p))});
    window.VaultBuddyQuality={inspectSession,showSession,getImportReport:()=>deep(importReport),getStats:()=>({issueRuns,issueHits}),hasUnsavedTake,rulerStep};
  }
  return {init,rulerStep,focusToken,restoreFocus,syncFilms,releaseSessionMedia,hasUnsavedTake,beginProjectSave,cancelProjectSave,endProjectSave,recordSaveRequest,inspectSession,showSession,renderImportSummary,showImportReport,observePeak,updateSplitState,
    get saveJob(){return saveJob;},get importJob(){return importJob;},set importJob(v){importJob=v;},get importReport(){return importReport;},set importReport(v){importReport=v;},
    cacheIssues(key,compute){if(key===issueKey){issueHits++;return issueCache.slice();}issueRuns++;issueKey=key;issueCache=compute();return issueCache.slice();},dialogFocus};
})();

// Frame and media metadata are checked before a workspace can replace the current one.
const qualityValidate = validate;
validate = function(p) {
  qualityValidate(p);
  if(!idSafe(p.id))throw new Error('Invalid project identifier.');
  const assets=new Map(p.assets.map(a=>[a.id,a]));
  for(const a of p.assets){
    for(const k of ['width','height'])if(a[k]!==undefined&&(!Number.isSafeInteger(a[k])||a[k]<0||a[k]>16384))throw new Error('Invalid source dimensions. Use finite pixel dimensions up to 16384.');
    if(a.size!==undefined&&(!Number.isSafeInteger(a.size)||a.size<0||a.size>512*1024*1024))throw new Error('Invalid source file size.');
    if(a.last_modified!==undefined&&(!Number.isFinite(a.last_modified)||a.last_modified<0))throw new Error('Invalid source modification time.');
    if(a.linked_asset!==undefined){
      const root=assets.get(a.linked_asset);
      if(!idSafe(a.linked_asset)||!root||root===a||root.linked_asset||root.builtin||a.kind!=='audio'||a.duration_ms!==root.duration_ms)throw new Error('Invalid linked audio source. Reconnect a direct original; cyclic or missing links are not supported.');
    }
  }
  return p;
};

// Measure geometry once per synchronous timeline render. Never cache across edits or resize.
let qualityTimelineMetrics=null;
const qualityPps=pps,qualityLaneWidth=laneWidth,qualityLabelWidth=labelWidth,qualitySpan=span;
pps=()=>qualityTimelineMetrics?.scale??qualityPps();
laneWidth=()=>qualityTimelineMetrics?.width??qualityLaneWidth();
labelWidth=()=>qualityTimelineMetrics?.label??qualityLabelWidth();
span=()=>qualityTimelineMetrics?.total??qualitySpan();
const qualityTimeline = renderTimeline;
renderTimeline = function(){
  const previous=document.activeElement,token=$('#timelineContent')?.contains(previous)?Quality.focusToken(previous):null;
  Quality.syncFilms();
  qualityTimelineMetrics={scale:qualityPps(),width:qualityLaneWidth(),label:qualityLabelWidth(),total:qualitySpan()};
  try{qualityTimeline();}finally{qualityTimelineMetrics=null;}
  Quality.restoreFocus(token,previous,$('#ruler'));
};
const qualityLibrary=renderLibrary;
renderLibrary=function(){qualityLibrary();Quality.renderImportSummary();};
const qualityTransport=updateTransport;
updateTransport=function(){qualityTransport();Quality.updateSplitState();};

// Exact graph keys avoid stale checks even for restored documents with reused revision numbers.
const qualityCollect=collectIssues;
collectIssues=function(){
  const key=JSON.stringify([documentToken(),projectDownloadToken,storageWarning,Quality.hasUnsavedTake(),[...media.keys()].sort(),[...runtimes.entries()].filter(([,r])=>r.failed).map(([id])=>id)]);
  return Quality.cacheIssues(key,()=>{
    const list=qualityCollect();
    const cfg=captionConfig();
    if(captionView().length&&(!cfg.enabled||!cfg.burn_in))list.push({id:'captions-excluded',severity:'warning',title:'Captions will not appear in the rendered video',detail:'Caption text exists, but display or burn-in is off. Enable both for visible captions, or download an SRT and attach it separately.',label:'Review caption settings',run:()=>{ui.tab='captions';ui.captionSettings=true;render();}});
    const visible=project.clips.filter(c=>track(c.track_id)?.kind==='video'&&track(c.track_id).visible);
    if(visible.length&&visible.every(c=>c.opacity===0))list.push({id:'transparent-video',severity:'warning',title:'All visible video clips are fully transparent',detail:'Every visible footage clip has zero opacity. This may be intentional: teaching annotations and captions can still appear. Review the encoded result.',label:'Inspect video opacity',run:()=>{selectClip(visible[0].id,true);showProperty('layout');}});
    if(Quality.hasUnsavedTake())list.push({id:'uncommitted-take',severity:'warning',title:'A webcam take is not yet part of the project',detail:'Add the take to the timeline or save the original recording before closing.',label:'Review webcam take',run:openWebcam});
    return list;
  });
};

const qualityCloseDialog=closeDialog;
closeDialog=function(d){if(d?.id==='projectSaveDialog'&&Quality.saveJob){Quality.cancelProjectSave();return;}return qualityCloseDialog(d);};
// Native dialogs trap focus. Remember a stable fallback when rendering replaces their opener.
const qualityOpenDialog=openDialog;
openDialog=function(id){const d=typeof id==='string'?$(id):id;Quality.dialogFocus.set(d,{element:document.activeElement,token:Quality.focusToken(document.activeElement)});return qualityOpenDialog(id);};
document.addEventListener('close',ev=>{
  const d=ev.target;if(!d.matches?.('dialog.dialog'))return;
  const saved=Quality.dialogFocus.get(d);if(!saved)return;
  queueMicrotask(()=>{
    if(document.querySelector('dialog[open]')||window.VaultBuddyGuide?.getState().active)return;
    if(saved.element?.isConnected&&saved.element.getClientRects().length)saved.element.focus({preventScroll:true});
    else Quality.restoreFocus(saved.token,saved.element,$('#playButton'));
  });
},true);
document.addEventListener('cancel',ev=>{if(ev.target.id==='projectSaveDialog'&&Quality.saveJob){ev.preventDefault();Quality.cancelProjectSave();}},true);

// Imports retain a readable result for every file, instead of overwriting one toast per item.
importFiles=async function(files){
  if(ui.importing||ui.exporting||projectBusy)return;
  const all=[...files];if(!all.length)return;
  ui.importing=true;pause();
  const id=project.id,job={total:all.length,index:0,cancelled:false};Quality.importJob=job;Quality.importReport=[];
  try{
    for(const f of all){
      job.index++;Quality.renderImportSummary();
      if(job.cancelled){Quality.importReport.push({name:f.name,status:'cancelled',message:'Not imported. Existing successful imports are kept.'});continue;}
      let a=null;
      try{
        if(project.assets.length>=200)throw new Error('The 200-source project limit is reached. Remove unused sources or start another project.');
        const info=await inspectFile(f);
        if(job.cancelled){Quality.importReport.push({name:f.name,status:'cancelled',message:'Stopped before adding this file.'});continue;}
        if(project.id!==id)throw new Error('Project changed during import. Please choose the file again.');
        a={id:uid('asset'),name:f.name,original_name:f.name,size:f.size,last_modified:f.lastModified||0,...info};
        await attachFile(a,f);
        if(job.cancelled||project.id!==id)throw new Error('Import stopped before the file was added.');
        if(!change(()=>project.assets.push(a),'Import media'))throw new Error('This media could not be added to the project.');
        cacheFile(a,f);Quality.importReport.push({name:f.name,status:'added',message:'Available in Media. Use + or drag it onto a compatible track.'});
      }catch(e){
        if(a&&!asset(a.id)){const m=media.get(a.id);if(m?.url)URL.revokeObjectURL(m.url);media.delete(a.id);thumbs.delete(a.id);}
        Quality.importReport.push({name:f.name,status:job.cancelled?'cancelled':'skipped',message:e.message==='Choose an audio or video file.'?'Choose a supported video, audio or image file.':e.message});
      }
      Quality.renderImportSummary();
    }
  }finally{
    ui.importing=false;Quality.importJob=null;render();
    const n=Quality.importReport.filter(r=>r.status==='added').length;
    toast(`Import finished: ${n} added, ${Quality.importReport.length-n} skipped or cancelled. Review the report in Media.`);
  }
  return deep(Quality.importReport);
};
const qualityNew=newEmptyProject,qualityReset=resetDemo,qualityOpenFile=openProjectFile;
newEmptyProject=function(){if(relinkBusy){toast('Finish reconnecting originals before replacing this project.');return;}if(ui.importing){toast('Finish or stop the media import before replacing this project.');return;}qualityNew();};
resetDemo=function(){if(relinkBusy){toast('Finish reconnecting originals before replacing this project.');return;}if(ui.importing){toast('Finish or stop the media import before replacing this project.');return;}qualityReset();};
openProjectFile=async function(f){if(relinkBusy){toast('Finish reconnecting originals before opening another project.');return;}if(ui.importing){toast('Finish or stop the media import before opening another project.');return;}return qualityOpenFile(f);};

window.addEventListener('click',ev=>{
  const b=ev.target.closest('[data-quality]');if(!b||b.disabled)return;
  ev.preventDefault();ev.stopImmediatePropagation();
  const action=b.dataset.quality;
  if(action==='cancel-save'){Quality.cancelProjectSave();return;}
  if(action==='cancel-import'){if(Quality.importJob)Quality.importJob.cancelled=true;Quality.renderImportSummary();return;}
  if(ui.exporting||projectBusy)return;
  if(action==='session')Quality.showSession();
  if(action==='session-save'){$('#sessionDialog').close();downloadProject();}
  if(action==='session-reconnect'){$('#sessionDialog').close();showRelinkDialog();}
  if(action==='import-report')Quality.showImportReport();
  if(action==='dismiss-import'){Quality.importReport=[];Quality.renderImportSummary();}
},true);
window.addEventListener('keydown',ev=>{
  const target=ev.target;
  if(target.matches?.('[data-quality][role=button]')&&['Enter',' '].includes(ev.key)){ev.preventDefault();ev.stopImmediatePropagation();target.click();return;}
  if(target.id==='ruler'&&!document.querySelector('dialog[open]')&&!ui.exporting&&['ArrowUp','ArrowDown','PageUp','PageDown'].includes(ev.key)){
    ev.preventDefault();ev.stopImmediatePropagation();
    const delta=ev.key==='ArrowUp'?1000/30:ev.key==='ArrowDown'?-1000/30:ev.key==='PageUp'?1000:-1000;
    seek(ui.time+delta);
  }
},true);
document.addEventListener('visibilitychange',()=>{if(document.hidden&&!projectBusy&&!drag&&!previewDrag&&!batchDrag)persistRecovery();});

