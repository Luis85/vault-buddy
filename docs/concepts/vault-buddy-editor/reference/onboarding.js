/* Guided onboarding. UI-only guidance; never invokes edit, import, capture, render or save commands.
 * Progress belongs to the learner/device, not to an editable project or rendered product.
 * No dependencies, telemetry, network requests, timeouts that advance lessons, or mandatory tasks.
 */
const Guide = (() => {
  const KEY = 'vault-buddy.editor.onboarding.v1';
  const SCHEMA = 'vault-buddy-onboarding/1';
  const CHAPTERS = [
    {id:'orient',title:'Find your way',subtitle:'Media, preview & timeline',icon:'compass'},
    {id:'edit',title:'Make the first edit',subtitle:'Select, split, undo & arrange',icon:'scissors'},
    {id:'layers',title:'Build your scene',subtitle:'Tracks, webcam & layout',icon:'layers'},
    {id:'polish',title:'Smooth the edges',subtitle:'Fades & audio levels',icon:'fade'},
    {id:'teach',title:'Make it easy to follow',subtitle:'Callouts, captions & chapters',icon:'text'},
    {id:'finish',title:'Save your work. Share a video.',subtitle:'Checks, editable projects & outputs',icon:'save'},
    {id:'return',title:'Come back anytime',subtitle:'Your projects & this guide',icon:'bookmark'}
  ];
  const STEPS = [
    {id:'welcome',chapter:'orient',title:'A recording is just the beginning.',target:'.projectbar',focus:'#libraryToggle',label:'Your tutorial workspace',body:'Turn a screen recording into a short tutorial. Media lives on the left, your preview is in the middle, and the timeline below controls what plays when.',tip:'The guide only changes the view and selection. It never applies edits for you. Actions you choose in the editor do affect this project.',visual:'workflow'},
    {id:'media',chapter:'orient',title:'Start with your source material.',target:'#library [data-action="import"]',focus:'#library [data-action="import"]',label:'Import video, audio or an image',prepare:'media',body:'Bring in a screen recording, narration, music or an image. Originals stay unchanged. Drag media to a compatible track, or use its + button.',tip:'The built-in project is ready to explore. Importing your own file is optional.',task:'Open the import picker, or use the sample and continue.',action:'[data-action="import"]'},
    {id:'preview',chapter:'orient',title:'See your edit in motion.',target:'.transport',focus:'#playButton',label:'Playback controls',body:'Press Play to watch the assembled timeline. The time readout is the position in your edit, not in the original recording. Click the timeline ruler to jump to another moment.',tip:'The speed next to the speaker changes preview playback only. It does not change the exported video.',task:'Play a few seconds, then pause.',action:'#playButton',keys:['Space','← / →']},
    {id:'timeline',chapter:'orient',title:'Time runs from left to right.',target:'.timeline-toolbar',focus:'#zoomRange',label:'Timeline controls',body:'Each rectangle is a clip. The vertical playhead shows the current moment. Zoom into short details, fit the whole edit, or drag the divider to make more room for tracks.',tip:'Video layers stack from top to bottom; audio layers mix together. Higher video tracks appear in front.',task:'Try timeline zoom or Fit.',action:'[data-action="fit"], [data-action="zoomIn"], [data-action="zoomOut"], #zoomRange',keys:['F']},
    {id:'select',chapter:'edit',title:'Choose the clip you want to change.',target:'@clip',focus:'@clip',label:'Selected timeline clip',prepare:'video',body:'Click a clip to select it. Its settings appear in Properties on the right. Shift-click adds another clip to the selection. A locked track protects its clips from editing.',tip:'Selection is not an edit. Use Edit actions → Clear selection or V to deselect without changing the video.',task:'Select a different clip, or keep this one.',action:'[data-clip]'},
    {id:'split',chapter:'edit',title:'Cut a clip, not the original.',target:'#splitButton',focus:'#splitButton',label:'Split at the playhead',prepare:'cut',body:'Put the playhead inside a selected clip, then choose Split. You get two independently editable pieces; no source frames are deleted. Remove an unwanted piece only after checking the delete mode.',tip:'Leave gap keeps other clips in place. Ripple this track closes the gap on this track only, which can change its alignment with other tracks.',task:'Optional edit: split here, then try Undo in the next step.',action:'#splitButton',keys:['S'],warning:true},
    {id:'undo',chapter:'edit',title:'You can take an edit back.',target:'#undoButton',focus:'#undoButton',label:'Undo and redo',body:'Undo reverses your most recent edit. Redo brings it back. This applies to cuts, moved clips, added teaching layers and property changes.',tip:'Undo reverses the most recent actual edit, not a tutorial step. If Undo is unavailable, there is nothing to undo yet.',task:'Use Undo only if you want to reverse the last edit.',action:'#undoButton',keys:['Ctrl / ⌘ Z'],warning:true},
    {id:'arrange',chapter:'edit',title:'Put each moment in the right place.',target:'.inspector',focus:'[data-prop="clipStart"]',label:'Clip timing in Properties',prepare:'properties',body:'Drag a clip horizontally to change when it starts, or use its numeric timing fields. Drag an edge to trim. Compatible tracks accept a vertical move; original media stays untouched.',tip:'Group related clips before moving them to preserve their relative timing. Keep snapping on when you want edges to line up.',task:'Explore the timing fields. Changing a value edits this project.',action:'[data-prop="clipStart"], [data-prop="sourceIn"], [data-prop="sourceOut"]',warning:true},
    {id:'context',chapter:'edit',title:'The action you need is right here.',target:'.timeline-toolbar [data-editor="more"]',focus:'.timeline-toolbar [data-editor="more"]',label:'More actions / right-click menu',prepare:'video',body:'Right-click a clip, annotation, track or media item for actions specific to that object. Edit actions in the timeline opens the same kind of menu without a right click.',tip:'When you right-click a clip, time-specific commands use the position you clicked. Disabled items explain what needs to change first.',task:'Open Edit actions, explore the menu, then close it with Escape.',action:'[data-editor="more"]',context:true,keys:['Shift F10','Esc']},
    {id:'tracks',chapter:'layers',title:'Give each layer its own track.',target:'[data-action="trackMenu"]',focus:'[data-action="trackMenu"]',label:'Add a video or audio track',body:'Use a video track for a screen recording, an image or a presenter. Use audio tracks for independent narration and music. Track controls handle visibility, mute, solo and locking.',tip:'Adding a track is optional. The sample already has multiple video and audio tracks.',task:'Open Add track to see the choices. You do not need to add one.',action:'[data-action="trackMenu"]'},
    {id:'webcam',chapter:'layers',title:'Add a face to the explanation.',target:'.webcam-entry',focus:'.webcam-entry',prepare:'media',label:'Record a webcam overlay',body:'Record a presenter take and place it above your screen recording. Review or retake it before adding it. It remains an independent clip that you can trim and position.',tip:'Opening the dialog does not activate your camera. This guide never requests camera or microphone permission. Close the dialog to return here.',task:'Optional: inspect the recording options. A camera is not needed for this guide.',action:'[data-action="webcam"]'},
    {id:'layout',chapter:'layers',title:'Keep the presenter out of the way.',target:'.inspector',focus:'[data-property-tab="layout"]',label:'Overlay layout controls',prepare:'layout',body:'Select the presenter or another video layer. Drag it in the preview; drag a corner to resize it. Layout offers numeric sizing, placement presets, framing and crop controls.',tip:'Choose a corner that does not cover the button, text or detail you are teaching. Review the crop before exporting.',task:'Inspect Layout, or reposition a layer if you are ready to edit.',action:'[data-prop], [data-preset]',warning:true},
    {id:'fades',chapter:'polish',title:'Give clips a softer start and end.',target:'.inspector',focus:'[data-prop="fadeIn"]',label:'Fade controls',prepare:'fades',body:'Set Fade in and Fade out, or drag the gold handles on a timeline clip. Video fades reveal the layer beneath; audio fades change volume. A preset is a quick starting point.',tip:'An edge fade is not a crossfade. A crossfade overlaps adjacent clips and shortens only their track. Check synchronization afterward.',task:'Optional edit: try a 0.5-second fade. Undo remains available.',action:'[data-fade-preset], [data-prop="fadeIn"], [data-prop="fadeOut"]',warning:true},
    {id:'audio',chapter:'polish',title:'Make the voice easy to hear.',target:'[data-action="mixer"]',focus:'[data-action="mixer"]',label:'Audio mixer',body:'Balance narration, source audio and music in the mixer. Track mute and solo affect your rendered video. The speaker beside Play only changes what you hear while monitoring.',tip:'Listen to the actual rendered file before sharing. The sample sounds are synthesized cues and ambience, not a recorded voice.',task:'Open the mixer. Close it when you are ready to continue.',action:'[data-action="mixer"]'},
    {id:'callouts',chapter:'teach',title:'Point to the next action.',target:'.toolstrip',focus:'.toolstrip [data-effect="arrow"]',label:'Teaching tools',prepare:'video',body:'Use the single preview header for text, arrows, highlights and zoom. More tools contains spotlights, numbered steps and privacy covers. Add them at the playhead. Annotations follow their source clip when you rearrange it.',tip:'Select an annotation to change its text, style, position and timing. One clear cue is often enough for a single action.',task:'Optional edit: add an arrow or another teaching cue.',action:'[data-effect]',warning:true},
    {id:'captions',chapter:'teach',title:'Let viewers follow without sound.',target:'.sidebar',focus:'[data-tab="captions"]',label:'Captions library',prepare:'captions',body:'Write captions or import an SRT or plain WebVTT file, then check text and timing against the selected clip. Caption cues remain linked to their source footage.',tip:'Captions are manual or imported; automatic transcription is not connected. Readability checks flag dense or overlapping captions, but still review them yourself.',task:'Explore caption options. Adding captions is optional.',action:'[data-editor="caption-add"], [data-editor="caption-import"]'},
    {id:'chapters',chapter:'teach',title:'Make the tutorial easy to revisit.',target:'.sidebar',focus:'[data-action="marker"]',label:'Tutorial chapters and companion note',prepare:'guide',body:'Add chapter markers for important steps. The companion Markdown note can embed the video and list timestamps, making the tutorial easier to find and use in Obsidian.',tip:'Chapters describes your video. Help opens this application walkthrough. They are separate.',task:'Preview the companion note without changing your project.',action:'[data-action="note"]'},
    {id:'checks',chapter:'finish',title:'Catch problems before you share.',target:'#issuesButton',focus:'#issuesButton',label:'Before-you-share checks',body:'Checks points you to missing media, uncovered gaps, audio concerns and caption issues. Each item explains the problem and offers a relevant next action.',tip:'Checks is a review aid, not a guarantee. Watch and listen to the encoded video, especially when it contains private information.',task:'Open Checks, inspect its findings, then close it.',action:'#issuesButton'},
    {id:'save',chapter:'finish',title:'Save the project. Keep the options.',target:'.header-actions [data-action="downloadProject"]',focus:'.header-actions [data-action="downloadProject"]',label:'Save editable project',body:'Save project keeps the editable workspace without rendering. A portable project includes available originals, timeline, teaching layers and render history so you can continue later.',tip:'Saving here downloads a project file. Keep that file somewhere safe. Browser recovery is not a backup, and a portable project may include uncensored originals.',task:'Open Save project to inspect the formats. No download is required to finish the guide.',action:'.header-actions [data-action="downloadProject"]',keys:['Ctrl / ⌘ S'],visual:'project'},
    {id:'render',chapter:'finish',title:'A video is a product of the project.',target:'.header-actions [data-action="save"]',focus:'.header-actions [data-action="save"]',label:'Render a separate video',body:'Render video creates a watchable output, not a replacement for the project. You can review a short range, play the actual output, then refine your edit and render another version.',tip:'Browser rendering here is a real-time review path with a three-minute limit. It does not write directly into a vault. Keep the editable project for later changes.',task:'Open Render video to inspect settings. You do not need to render anything.',action:'.header-actions [data-action="save"]'},
    {id:'products',chapter:'return',title:'Keep working after the first export.',target:'.sidebar',focus:'#library [data-action="openProject"],#projectMenuButton',label:'Project and rendered products',prepare:'project',body:'Open Project in the app header to find workspace files and rendered products. This keeps the editable workspace separate from rendered products. Open a saved project to continue, or inspect the edit snapshot behind an earlier output.',tip:'Missing originals can be reconnected without rebuilding your edit. An exported video by itself does not preserve separate tracks or annotations.',task:'Look through Project, then continue.',action:'[data-tab="project"]'},
    {id:'help',chapter:'return',title:'Pick up here whenever you need.',target:'#editorHelp',focus:'#editorHelp',label:'Help & learning center',body:'Help is always here. Resume a paused walkthrough at the same step, jump to a chapter, search a quick answer, or revisit shortcuts. You can repeat any lesson.',tip:'Escape or Pause guide dismisses the overlays immediately. Your guide progress is separate from project saves.',task:'Finish the walkthrough, or pause and return later.',keys:['F1','Esc'],visual:'finish'}
  ];
  const ANSWERS = [
    ['Where are the main controls?','Project in the header contains file commands and rendered products. Media has Import and Webcam. The preview header contains common callouts; More tools has spotlights, steps and privacy covers. Edit actions beside the timeline contains selection commands. Fades, sound, speed and color are in Properties. View options restores hidden panels.','welcome'],
    ['Will this guide change my edit?','Starting, skipping or finishing lessons only changes the view, selection and playhead. The guide never splits, deletes, records, downloads or renders on your behalf. Any editing control you choose still works normally on your current project.','welcome'],
    ['How do I pause and resume?','Choose Pause guide, use the close button, or press Escape when no editor dialog or menu is open. Open Help and choose Resume walkthrough. The current step is remembered; no need to start again.','help'],
    ['Where is my progress stored?','Only on this browser/device when local storage is available. It is separate from the project and is not uploaded. Moving the HTML, clearing browser data or private browsing can lose it. Save a guide-progress file below as a portable backup.','help'],
    ['Why can’t I hear my audio?','Open the mixer. Check clip and track mute, solo, and levels. The speaker near Play mutes monitoring only; exported audio uses the actual track settings. Listen to the encoded file as well as the preview.','audio'],
    ['What is the difference between Save and Render?','Save project preserves the editable composition; a portable project also contains available original media. Render creates a separate flattened video. Keep your saved project to change timing, text or tracks later.','save'],
    ['Why did the tracks go out of sync?','Ripple deletion and a crossfade affect only their track. Group related clips before moving them, check alignment afterward, and undo an unexpected shift.','arrange'],
    ['Why is my camera unavailable?','Camera and microphone access requires a supported browser, a secure context and permission. Another application or system policy may block it. You can complete this guide without granting any device access.','webcam'],
    ['Why is a tutorial control unavailable?','Some controls need a selected clip, compatible media or an unlocked track. The guide explains the dependency and keeps Next available; it will not unlock, import or create anything for you.','select'],
    ['How do I avoid sharing private source content?','Review the actual encoded output. A stationary cover is not motion tracking or a redaction guarantee. Portable projects contain originals and may include material hidden or trimmed out of the rendered video. Share the intended product, not the full workspace, when originals must remain private.','checks']
  ];
  let state, storageOK = false, active = false, minimized = false, suspended = false, ready = false;
  let target = null, described = null, oldDescription = null, lastFocus = null, layoutFrame = 0, observedStep = '', hubTab = 'tour';
  let resizeObserver, appObserver, modalObserver, lastDialog = null, trackedProject = null, outOfView = false;
  const q = s => document.querySelector(s), qa = s => [...document.querySelectorAll(s)];
  const index = id => STEPS.findIndex(s => s.id === id);
  const step = () => STEPS[Math.max(0,index(state.step))];
  const fresh = () => ({schema:SCHEMA,step:'welcome',status:'new',reviewed:[],tried:[],welcomeDismissed:false,dim:true,updatedAt:null});
  const own = (v,k) => Object.prototype.hasOwnProperty.call(v,k);
  function parse(value) {
    if (!value || value.schema !== SCHEMA || typeof value !== 'object' || Array.isArray(value)) throw new Error('This is not a supported guide-progress file.');
    if (!Array.isArray(value.reviewed)||!Array.isArray(value.tried)||value.reviewed.length>300||value.tried.length>300) throw new Error('Guide progress is not valid.');
    const clean = list => [...new Set(list.filter(id=>typeof id==='string'&&index(id)>=0))];
    const result={...fresh(),reviewed:clean(value.reviewed),tried:clean(value.tried),welcomeDismissed:value.welcomeDismissed===true,dim:value.dim!==false};
    result.step=index(value.step)>=0?value.step:(STEPS.find(s=>!result.reviewed.includes(s.id))?.id||'welcome');
    result.status=['new','paused','completed'].includes(value.status)?value.status:'paused';
    if(result.status==='completed'&&result.reviewed.length!==STEPS.length)result.status='paused';
    result.updatedAt=typeof value.updatedAt==='string'&&value.updatedAt.length<40?value.updatedAt:null;
    return result;
  }
  function read() {
    state=fresh();
    try {
      const store=window.localStorage, probe=KEY+'.probe'; store.setItem(probe,'1'); storageOK=store.getItem(probe)==='1'; store.removeItem(probe);
      const raw=store.getItem(KEY); if(raw&&raw.length<65536)state=parse(JSON.parse(raw));
    } catch { /* An opaque origin or denied storage must never break the editor. */ }
  }
  function persist() {
    state.updatedAt=new Date().toISOString();
    try { window.localStorage.setItem(KEY,JSON.stringify(state));storageOK=true; } catch {storageOK=false;}
    updateHelp();
    const n=q('#guidePersistence');if(n)n.textContent=persistenceText();
    const n2=q('#guideCoachStorage');if(n2)n2.textContent=storageOK?'Progress remembered on this device':'Session only · back up progress in Help';
  }
  function persistenceText() { return storageOK?'Progress is stored on this browser/device. Clearing browser data can remove it.':'Persistent storage is unavailable. Progress lasts in this session only; save a progress file to resume after closing.'; }
  function updateHelp() {
    const b=q('#editorHelp');if(!b)return;
    b.classList.toggle('has-guide-progress',state.status==='paused');
    b.setAttribute('title',active?'Help & guide contents (F1)':state.status==='paused'?`Resume guide: ${step().title} (F1)`:'Help & learning center (F1)');
    b.setAttribute('aria-label',state.status==='paused'?'Help — resume walkthrough':'Help & learning center');
  }
  function icon(name) { return svg(name); }
  function diagram(type) {
    if(type==='workflow')return '<div class="guide-flow" aria-label="Capture, edit, then create a video"><span>'+icon('video')+'Capture</span><i>→</i><span class="current">'+icon('scissors')+'Edit</span><i>→</i><span>'+icon('file')+'Share</span></div>';
    if(type==='project')return '<div class="guide-project-diagram"><span>'+icon('folder')+'<b>Editable project</b><small>Originals + your decisions</small></span><i>→</i><span>'+icon('video')+'<b>Rendered video</b><small>A separate product</small></span></div>';
    if(type==='finish')return '<div class="guide-flow"><span>'+icon('bookmark')+'Pause</span><i>→</i><span class="current">'+icon('info')+'Help</span><i>→</i><span>'+icon('play')+'Resume</span></div>';
    return '';
  }
  function renderHub() {
    const panel=q('#guideHubBody'),reviewed=state.reviewed.length,percent=Math.round(reviewed/STEPS.length*100),s=step();
    q('#guideHubEyebrow').textContent=state.status==='completed'?'WALKTHROUGH REVIEWED':'YOUR EDITOR, EXPLAINED';
    q('#guideHubHeading').textContent=state.status==='completed'?'A reference whenever you need it.':'From recording to a clear tutorial.';
    q('#guideHubSub').textContent='One explained control at a time. Skip anything. Come back anytime.';
    q('#guideHubAction').innerHTML=icon(state.status==='new'?'play':'bookmark')+(state.status==='new'?'Start walkthrough':state.status==='completed'?'Revisit walkthrough':'Resume walkthrough');
    q('#guideHubAction').dataset.guide=state.status==='completed'?'revisit':'resume';
    q('#guideResumeDetail').textContent=state.status==='paused'?`Continue at step ${index(s.id)+1} · ${s.title}`:state.status==='completed'?`${reviewed} of ${STEPS.length} steps reviewed. Repeat any chapter below.`:`${CHAPTERS.length} chapters · ${STEPS.length} short steps · no setup needed`;
    q('#guideProgressNumber').textContent=percent+'%';q('#guideProgressLabel').textContent=`${reviewed} / ${STEPS.length} reviewed`;
    q('#guideProgressRing').style.setProperty('--guide-progress',percent+'%');
    q('#guideProgressRing').setAttribute('aria-label',`${reviewed} of ${STEPS.length} steps reviewed`);
    qa('[data-guide-hub-tab]').forEach(b=>{b.classList.toggle('selected',b.dataset.guideHubTab===hubTab);b.setAttribute('aria-pressed',String(b.dataset.guideHubTab===hubTab));});
    if(hubTab==='tour')panel.innerHTML=`<div class="guide-list-heading"><div><h3>Pick up a skill.</h3><p>Start at the beginning or jump straight to what you need.</p></div><button class="guide-link" data-guide="restart">Start over</button></div><div class="guide-chapters">${CHAPTERS.map((c,i)=>{const steps=STEPS.filter(s=>s.chapter===c.id),n=steps.filter(s=>state.reviewed.includes(s.id)).length;return `<article class="guide-chapter ${n===steps.length?'is-reviewed':''}"><button class="guide-chapter-main" data-guide-jump="${steps[0].id}" aria-label="Open chapter ${i+1}: ${esc(c.title)}"><span class="guide-chapter-icon">${icon(n===steps.length?'check':c.icon)}</span><span class="guide-chapter-copy"><b>${esc(c.title)}</b><small>${esc(c.subtitle)}</small></span><span class="guide-chapter-count">${n}/${steps.length}</span>${icon('chevronRight')}</button><details><summary>See ${steps.length} steps</summary><div class="guide-step-list">${steps.map(t=>`<button data-guide-jump="${t.id}" ${t.id===state.step?'aria-current="step"':''}><span class="guide-checkmark">${state.reviewed.includes(t.id)?'✓':String(index(t.id)+1).padStart(2,'0')}</span><span>${esc(t.title)}</span>${t.id===state.step&&state.status==='paused'?'<small>Resume here</small>':''}</button>`).join('')}</div></details></article>`;}).join('')}</div><div class="guide-safety">${icon('lock')}<p><b>Your edit stays yours.</b> The guide does not automatically cut, record or render. You can simply read and choose Next. Controls you use still edit the current project.</p></div>`;
    else if(hubTab==='answers')panel.innerHTML=`<label class="guide-search">${icon('search')}<input id="guideSearch" type="search" placeholder="Search audio, saving, camera…" aria-label="Search quick answers" autocomplete="off"></label><div class="guide-answers">${ANSWERS.map(([title,text,id])=>`<details class="guide-answer" data-guide-search="${esc((title+' '+text).toLowerCase())}"><summary>${esc(title)}</summary><p>${esc(text)}</p><button class="guide-link" data-guide-jump="${id}">Show me in the editor ${icon('chevronRight')}</button></details>`).join('')}</div><p id="guideNoResults" hidden>No matching answers. Try “save”, “audio” or “camera”.</p>`;
    else panel.innerHTML=`<div class="guide-list-heading"><div><h3>Keep your hands on the keyboard.</h3><p>The same controls are available without dragging or right-clicking.</p></div></div><div class="guide-key-table">${[['F1 or ?','Open Help & learning center'],['Esc','Pause the guide; menus and editor dialogs close first'],['F6','Switch between the guide and highlighted control'],['Tab / Shift Tab','Move between guide buttons; use Focus control to leave'],['Enter / Space','Activate the focused button'],['Space in editor','Play or pause'],['S in editor','Split selected clip at playhead'],['Ctrl / ⌘ Z','Undo the last edit'],['Ctrl / ⌘ S','Save the editable project'],['Shift F10','Open the selected object’s context menu'],['Shift + click','Select more than one clip']].map(([k,t])=>`<div><span>${esc(t)}</span><kbd>${esc(k)}</kbd></div>`).join('')}</div><button class="btn" data-guide="shortcut-help">All editor shortcuts</button>`;
    q('#guidePersistence').textContent=persistenceText();q('#guideDimOption').checked=state.dim;
  }
  function showHub() {
    if(!ready)return;
    if(ui.exporting||projectBusy){toast('Finish or cancel the active operation before opening the walkthrough.');return;}
    if(q('dialog[open]:not(#guideHub)')){toast('Close the current dialog before opening Help.');return;}
    lastFocus=document.activeElement;
    if(active)stop(false);
    q('#guideInvite').hidden=true;
    renderHub();q('#guideHub').showModal();q('#guideHubHeading').focus({preventScroll:true});
  }
  function closeHub() { if(q('#guideHub').open)q('#guideHub').close(); }
  function focusHelp() {q('#editorHelp')?.focus({preventScroll:true});}
  function showInvitation() {
    if(state.welcomeDismissed||state.status!=='new'||q('dialog[open]')||ui.exporting)return;
    q('#guideInvite').hidden=false; // No focus theft and no compulsory dialog on startup.
  }
  function dismissInvitation() {state.welcomeDismissed=true;persist();q('#guideInvite').hidden=true;}
  function getVideo(preferOverlay=false) {
    const videos=project.clips.filter(c=>asset(c.asset_id)?.kind==='video');
    if(preferOverlay)return videos.find(c=>c.w<.9)||videos[0]||null;
    const selected=selectedClip();return selected&&asset(selected.asset_id)?.kind==='video'?selected:videos.find(c=>c.w>=.9)||videos[0]||null;
  }
  function prepare(s) {
    if(window.VaultBuddyFocus) window.VaultBuddyFocus.prepareGuide(s.prepare);
    if(ui.review){ui.review=false;render();}
    if(['media','teach','captions','guide','project'].includes(s.prepare)) {
      ui.tab=s.prepare;$('#workspace').classList.remove('library-hidden');
      if(innerWidth<=800)$('#app').classList.add('library-drawer');
      render();
    } else if(innerWidth<=800)$('#app').classList.remove('library-drawer');
    if(['video','cut','properties','layout','fades','teach','captions'].includes(s.prepare)) {
      const c=getVideo(s.prepare==='layout');
      if(c){ui.selected={type:'clip',id:c.id};multiIds=[c.id];
        if(s.prepare==='cut'||ui.time<c.start_ms||ui.time>=end(c))ui.time=Math.round(c.start_ms+clipDuration(c)*.35);
      }
      if(['properties','layout','fades'].includes(s.prepare)) {
        ui.propertyTab=s.prepare;$('#workspace').classList.add('inspector-open');
      } else if(innerWidth<=800)$('#workspace').classList.remove('inspector-open');
      render();syncMedia();
    }
    if(['properties','layout','fades'].includes(s.prepare))$('#inspector').scrollTop=0;
    const el=resolve(s.target);if(el)el.scrollIntoView({block:'nearest',inline:'nearest',behavior:'instant'});
  }
  function resolve(selector) {
    if(selector==='@clip') {const c=selectedClip();return c?q(`[data-clip="${CSS.escape(c.id)}"]`):null;}
    return qa(selector).find(el=>el.getClientRects().length>0)||q(selector);
  }
  function visibleRect(el) {
    if(!el||!el.isConnected||el.getClientRects().length===0)return null;
    const r=el.getBoundingClientRect();let l=Math.max(6,r.left),t=Math.max(6,r.top),right=Math.min(innerWidth-6,r.right),bottom=Math.min(innerHeight-6,r.bottom);
    for(let p=el.parentElement;p&&p!==document.body;p=p.parentElement){const st=getComputedStyle(p),b=p.getBoundingClientRect();if(/hidden|auto|scroll|clip/.test(st.overflowX)){l=Math.max(l,b.left);right=Math.min(right,b.right);}if(/hidden|auto|scroll|clip/.test(st.overflowY)){t=Math.max(t,b.top);bottom=Math.min(bottom,b.bottom);}}
    return right-l>5&&bottom-t>5?{left:l,top:t,right,bottom,width:right-l,height:bottom-t}:null;
  }
  function cleanupTarget() {
    if(described){if(oldDescription===null)described.removeAttribute('aria-describedby');else described.setAttribute('aria-describedby',oldDescription);described.removeAttribute('data-guide-highlight');}
    described=null;oldDescription=null;target=null;resizeObserver?.disconnect();resizeObserver?.observe(q('#guideCoach'));
  }
  function setTarget(el) {
    if(el===target)return;cleanupTarget();target=el;
    if(el){described=el;oldDescription=el.getAttribute('aria-describedby');el.setAttribute('aria-describedby',[oldDescription,'guideTargetDescription'].filter(Boolean).join(' '));el.dataset.guideHighlight='true';resizeObserver?.observe(el);}
  }
  function renderCoach() {
    const s=step(),i=index(s.id),chapter=CHAPTERS.find(c=>c.id===s.chapter);
    q('#guideCoach').dataset.step=s.id;
    q('#guideStepCount').textContent=`${i+1} / ${STEPS.length}`;
    q('#guideChapterLabel').textContent=chapter.title;
    q('#guideCoachTitle').textContent=s.title;
    q('#guideCoachBody').textContent=s.body;
    q('#guideCoachVisual').innerHTML=diagram(s.visual);
    q('#guideCoachTip').textContent=s.tip;
    q('#guideTaskText').textContent=state.tried.includes(s.id)?'Control explored. Continue whenever you are ready.':s.task||'Read this step, then continue when you are ready.';
    q('#guideTask').classList.toggle('tried',state.tried.includes(s.id));
    q('#guideTask').classList.toggle('is-edit',!!s.warning&&!state.tried.includes(s.id));
    q('#guideTaskIcon').innerHTML=icon(state.tried.includes(s.id)?'check':s.warning?'edit':'cursor');
    q('#guideCoachKeys').innerHTML=(s.keys||[]).map(k=>`<kbd>${esc(k)}</kbd>`).join('');
    q('#guideBack').disabled=i===0;
    q('#guideNext').innerHTML=(i===STEPS.length-1?'Finish guide':'Next')+icon(i===STEPS.length-1?'check':'chevronRight');
    q('#guideMiniText').textContent=`${i+1}/${STEPS.length} · ${s.title}`;
    q('#guideProgressLine').style.width=((i+1)/STEPS.length*100)+'%';
    q('#guideTargetDescription').textContent=`Guide step ${i+1}: ${s.label}. ${s.body}`;
    q('#guideCoachStorage').textContent=storageOK?'Progress remembered on this device':'Session only · back up progress in Help';
    q('#guideLive').textContent=`Step ${i+1} of ${STEPS.length}. ${s.title}`;
    q('#guideTargetLabel').textContent=s.label;
    q('#guideStatusText').textContent='';
    outOfView=false;
    observedStep='';scheduleLayout();
  }
  function start(id=state.step) {
    if(ui.exporting||projectBusy||q('dialog[open]:not(#guideHub)')) {toast('Close the current dialog or finish the active operation before starting the guide.');return false;}
    if(index(id)<0)id='welcome';
    // Do not leave an obsolete guide-pause toast over a newly resumed lesson.
    // Preserve unrelated editor errors and notifications.
    if(q('#toast')?.textContent.startsWith('Guide paused.')) {clearTimeout(toastTimer);q('#toast').classList.remove('visible');}
    closeHub();dismissInvitation();pause();if(typeof closeContext==='function')closeContext();$('#menu').hidden=true;
    if(!active)lastFocus=document.activeElement;
    active=true;minimized=false;suspended=false;trackedProject=project.id;state.step=id;state.status='paused';persist();
    q('#guideLayer').hidden=false;q('#guideMini').hidden=true;
    prepare(step());renderCoach();scheduleLayout();q('#guideCoachTitle').focus({preventScroll:true});
    return true;
  }
  function stop(announce=true) {
    if(!active)return;
    active=false;minimized=false;suspended=false;cleanupTarget();
    q('#guideLayer').hidden=true;q('#guideMini').hidden=true;q('#guideTargetLabel').hidden=true;
    qa('.guide-dialog-note').forEach(n=>n.remove());lastDialog=null;persist();
    if(announce){q('#guideLive').textContent=`Guide paused at step ${index(state.step)+1}. Resume from Help.`;toast('Guide paused. Help → Resume walkthrough brings you back here.');focusHelp();}
  }
  function go(delta) {
    if(!active||suspended)return;
    if(delta>0&&!state.reviewed.includes(state.step))state.reviewed.push(state.step);
    const i=index(state.step)+delta;
    if(i>=STEPS.length){finish();return;}
    if(i<0)return;
    cleanupTarget();state.step=STEPS[i].id;pause();prepare(step());persist();renderCoach();q('#guideCoachTitle').focus({preventScroll:true});
  }
  function finish() {
    const all=state.reviewed.length===STEPS.length;stop(false);state.status=all?'completed':'paused';persist();
    renderHub();closeHub();q('#guideCompletionHeading').textContent=all?'You have the lay of the land.':'You reached the end.';
    q('#guideCompletionText').textContent=all?'The walkthrough is complete. Your editor is ready whenever you are. Help keeps every lesson within reach.':`${state.reviewed.length} of ${STEPS.length} steps reviewed. Skipped steps are still available in Help; you can return to any of them.`;
    q('#guideCompletion').showModal();q('#guideCompletionHeading').focus();
  }
  function markTried(id) {
    if(id!==state.step||state.tried.includes(id))return;
    state.tried.push(id);persist();if(!active)return;
    q('#guideTaskText').textContent='Control explored. Continue whenever you are ready.';q('#guideTask').classList.add('tried');q('#guideTask').classList.remove('is-edit');q('#guideTaskIcon').innerHTML=icon('check');
    q('#guideLive').textContent='Control explored. The guide will wait for you to choose Next.';
  }
  function focusTarget() {
    const s=step();let el=resolve(s.focus||s.target);
    if(!el||el.disabled||!visibleRect(el)){
      prepare(s);scheduleLayout();el=resolve(s.focus||s.target);
    }
    if(!el||el.disabled||!visibleRect(el)){q('#guideLive').textContent='This control is not available in the current project. You can continue to the next step.';return;}
    if(!el.matches('button,input,select,textarea,a,[tabindex]'))el.setAttribute('tabindex','-1');
    if(overlaps(el.getBoundingClientRect(),q('#guideCoach').getBoundingClientRect())||innerWidth<760)minimize();
    el.focus({preventScroll:false});q('#guideShade').hidden=true;
  }
  function minimize() {if(!active)return;minimized=true;q('#guideLayer').hidden=true;q('#guideMini').hidden=false;q('#guideMini [data-guide="expand"]').focus({preventScroll:true});}
  function expand() {if(!active)return;minimized=false;q('#guideMini').hidden=true;q('#guideLayer').hidden=false;scheduleLayout();q('#guideCoachTitle').focus({preventScroll:true});}
  function overlaps(a,b){return a&&b&&a.left<b.right&&a.right>b.left&&a.top<b.bottom&&a.bottom>b.top;}
  function scheduleLayout(){if(!active||layoutFrame)return;layoutFrame=requestAnimationFrame(()=>{layoutFrame=0;layout();});}
  function layout() {
    if(!active)return;
    if(project.id!==trackedProject){stop();toast('Project changed. The guide is paused; resume from Help when ready.');return;}
    const modal=q('dialog[open]');
    const menu=q('.context-panel')||(!$('#menu').hidden?$('#menu'):null);
    suspended=!!modal||!!menu||ui.exporting;
    if(modal&&modal!==lastDialog){qa('.guide-dialog-note').forEach(n=>n.remove());lastDialog=modal;const body=modal.querySelector('.dialog-body')||modal;const note=document.createElement('div');note.className='guide-dialog-note';note.innerHTML=`${icon('bookmark')}<span>Walkthrough is waiting. Close this window to continue.</span><button data-guide="pause">Pause guide</button>`;body.prepend(note);}
    if(!modal&&lastDialog){qa('.guide-dialog-note').forEach(n=>n.remove());lastDialog=null;}
    if(suspended){q('#guideLayer').hidden=true;q('#guideMini').hidden=true;return;}
    if(minimized){q('#guideLayer').hidden=true;q('#guideMini').hidden=false;return;}
    q('#guideLayer').hidden=false;q('#guideMini').hidden=true;
    const s=step(),el=resolve(s.target);setTarget(el);const r=visibleRect(el);outOfView=!r;
    const card=q('#guideCoach');
    card.style.maxHeight=Math.max(175,innerHeight-24)+'px';
    const width=Math.min(362,innerWidth-24);card.style.width=width+'px';
    let h=card.getBoundingClientRect().height;const w=width,gap=18,pad=12;
    let x=innerWidth-w-pad,y=Math.min(95,innerHeight-h-pad),placement='free';
    if(r){
      const cx=r.left+r.width/2,cy=r.top+r.height/2;
      const candidates=[
        {x:r.right+gap,y:clamp(cy-h/2,pad,innerHeight-h-pad),p:'right'},
        {x:r.left-w-gap,y:clamp(cy-h/2,pad,innerHeight-h-pad),p:'left'},
        {x:clamp(cx-w/2,pad,innerWidth-w-pad),y:r.bottom+gap,p:'below'},
        {x:clamp(cx-w/2,pad,innerWidth-w-pad),y:r.top-h-gap,p:'above'}
      ];
      // Prefer the outside edge for side panels; leave the preview usable when possible.
      const valid=candidates.filter(c=>c.x>=pad&&c.y>=pad&&c.x+w<=innerWidth-pad&&c.y+h<=innerHeight-pad);
      let winner=valid[0];
      if(menu&&valid.length){const mr=menu.getBoundingClientRect();winner=valid.find(c=>!overlaps({left:c.x,top:c.y,right:c.x+w,bottom:c.y+h},mr))||winner;}
      if(winner){x=winner.x;y=winner.y;placement=winner.p;}
      else {
        // Prefer a scrollable explanation to covering the very button being explained.
        // Navigation stays visible while only the explanation scrolls.
        const above=r.top-gap-pad,below=innerHeight-r.bottom-gap-pad;
        const room=Math.max(above,below);
        if(room>=240&&room<h){
          card.style.maxHeight=room+'px';h=card.getBoundingClientRect().height;
          y=below>=above?r.bottom+gap:r.top-gap-h;placement='compact-scroll';
        }else{y=r.top>innerHeight/2?pad:innerHeight-h-pad;placement='compact';}
      }
      const ring=q('#guideRing');ring.hidden=false;Object.assign(ring.style,{left:(r.left-4)+'px',top:(r.top-4)+'px',width:(r.width+8)+'px',height:(r.height+8)+'px'});
      const t=q('#guideTargetLabel');t.hidden=innerWidth<700||!!menu;t.style.left=clamp(r.left,8,innerWidth-Math.min(250,t.offsetWidth)-8)+'px';t.style.top=(r.top>34?r.top-30:r.bottom+10)+'px';
      const sh=qa('#guideShade > i');
      const values=[{left:0,top:0,width:innerWidth,height:Math.max(0,r.top-5)}, {left:0,top:r.top-5,width:Math.max(0,r.left-5),height:r.height+10}, {left:r.right+5,top:r.top-5,width:Math.max(0,innerWidth-r.right-5),height:r.height+10}, {left:0,top:r.bottom+5,width:innerWidth,height:Math.max(0,innerHeight-r.bottom-5)}];
      sh.forEach((e,i)=>Object.assign(e.style,Object.fromEntries(Object.entries(values[i]).map(([k,v])=>[k,v+'px']))));
    }else{q('#guideRing').hidden=true;q('#guideTargetLabel').hidden=true;}
    card.style.left=clamp(x,pad,Math.max(pad,innerWidth-w-pad))+'px';card.style.top=clamp(y,pad,Math.max(pad,innerHeight-h-pad))+'px';card.dataset.placement=placement;
    const targetFocus=resolve(s.focus||s.target),disabled=!!targetFocus?.disabled;
    const needsClip=['video','cut','properties','layout','fades','teach','captions'].includes(s.prepare)&&!getVideo();
    const status=needsClip?'No video clip in this project. Import one when ready, or keep reading.':!r?'This control is outside the current view. Choose Show control, or continue.':disabled?(s.id==='undo'?'Nothing to undo yet. You can move on.':'This control is unavailable in the current selection. You can move on.') :'';
    if(q('#guideStatusText').textContent!==status)q('#guideStatusText').textContent=status;
    q('#guideStatusText').hidden=!status;
    q('#guideFocus').textContent=!r?'Show control':'Focus control';q('#guideFocus').disabled=disabled||needsClip;
    const focused=document.activeElement,externalFocus=focused&&focused!==document.body&&!card.contains(focused)&&!el?.contains(focused)&&focused!==el;
    q('#guideShade').hidden=!state.dim||!r||!!menu||externalFocus;
  }
  function restart(){q('#guideRestart').showModal();q('#guideRestartCancel').focus();}
  function resetConfirmed(){stop(false);const dim=state.dim;state=fresh();state.dim=dim;state.welcomeDismissed=true;persist();q('#guideRestart').close();closeHub();start('welcome');}
  function exportProgress(){downloadBlob(new Blob([JSON.stringify(state,null,2)],{type:'application/json'}),'vault-buddy-guide-progress.vbguide.json');toast('Guide-progress download requested. This file does not contain your project or media.');}
  async function importProgress(file){if(!file)return;try{if(file.size>65536)throw new Error('Guide-progress files must be smaller than 64 KB.');const restored=parse(JSON.parse(await file.text()));stop(false);state=restored;state.welcomeDismissed=true;persist();renderHub();toast('Guide progress loaded. Choose Resume, or pick a chapter.');}catch(err){toast(err instanceof SyntaxError?'This file is not valid JSON guide progress.':err.message);}}
  function click(event) {
    const b=event.target.closest('[data-guide], [data-guide-jump], [data-guide-hub-tab]');
    if(b){event.preventDefault();event.stopImmediatePropagation();if(b.disabled)return;
      if(b.dataset.guideJump){start(b.dataset.guideJump);return;}
      if(b.dataset.guideHubTab){hubTab=b.dataset.guideHubTab;renderHub();return;}
      switch(b.dataset.guide){
        case 'hub':showHub();break;case 'resume':start();break;case 'revisit':start('welcome');break;case 'start':start('welcome');break;
        case 'dismiss-invite':dismissInvitation();break;case 'close-hub':closeHub();break;
        case 'next':go(1);break;case 'back':go(-1);break;case 'pause':stop();break;case 'contents':stop(false);showHub();break;
        case 'minimize':minimize();break;case 'expand':expand();break;case 'focus':focusTarget();break;
        case 'restart':restart();break;case 'cancel-restart':q('#guideRestart').close();break;case 'confirm-restart':resetConfirmed();break;
        case 'save-progress':exportProgress();break;case 'load-progress':q('#guideProgressInput').click();break;
        case 'finish-close':q('#guideCompletion').close();focusHelp();break;
        case 'finish-lessons':q('#guideCompletion').close();showHub();break;
        case 'hub-from-legacy':closeDialog(q('#helpDialog'));showHub();break;
        case 'shortcut-help':closeHub();openDialog('#helpDialog');break;
      }return;
    }
    if(!active||suspended)return;const s=step();if(s.action&&event.target.closest(s.action)){const id=s.id;setTimeout(()=>markTried(id),30);}
    if(active)scheduleLayout();
  }
  function key(event) {
    if(!ready)return;
    const ownModal=q('#guideHub[open],#guideCompletion[open],#guideRestart[open]');
    if(event.key==='F1'){event.preventDefault();event.stopImmediatePropagation();if(!q('dialog[open]:not(#guideHub)'))showHub();return;}
    if(!active&&!ownModal&&!event.target.closest('#guideInvite'))return;
    const card=q('#guideCoach'),inCoach=card.contains(event.target)||!!event.target.closest('#guideMini');
    if(active&&!q('dialog[open]')&&event.key==='Escape'){
      if(q('.context-panel')||!$('#menu').hidden||drag||previewDrag)return;
      event.preventDefault();event.stopImmediatePropagation();stop();return;
    }
    if(active&&!q('dialog[open]')&&event.key==='F6') {event.preventDefault();event.stopImmediatePropagation();if(inCoach)focusTarget();else {if(minimized)expand();card.querySelector('h2').focus({preventScroll:true});}return;}
    if(inCoach||ownModal||event.target.closest('#guideInvite')) {
      // Guide keystrokes must not reach the editor's single-letter edit shortcuts.
      event.stopImmediatePropagation();
      if(inCoach&&event.key==='Tab') {
        const root=event.target.closest('#guideMini')||card,controls=[...root.querySelectorAll('button:not(:disabled),[href],input:not(:disabled)')].filter(e=>e.getClientRects().length);
        const i=controls.indexOf(document.activeElement);
        if(event.shiftKey&&(i<=0)||!event.shiftKey&&(i===controls.length-1||i<0)){event.preventDefault();(event.shiftKey?controls.at(-1):controls[0])?.focus();}
      }
      // Native dialog Escape behavior is intentionally preserved.
    }
  }
  function init() {
    if(ready)return;read();
    Object.assign(ICONS,{minus:'<path d="M5 12h14"/>',compass:'<circle cx="12" cy="12" r="9"/><path d="m16 8-2 6-6 2 2-6Z"/>',book:'<path d="M12 5v15M3 4c4-1 6 0 9 2 3-2 5-3 9-2v14c-4-1-6 0-9 2-3-2-5-3-9-2Z"/>'});
    const help=q('.header-actions [data-action="help"]');help.id='editorHelp';help.className='btn guide-help-button';help.innerHTML=icon('book')+'<span>Help</span><i class="guide-resume-dot" aria-hidden="true"></i>';
    const guideTab=q('[data-tab="guide"]');guideTab.textContent='Chapters';guideTab.title='Chapters in your video and its companion note';
    
    document.body.insertAdjacentHTML('beforeend', `
      <section id="guideInvite" class="guide-invite" aria-labelledby="guideInviteTitle" hidden>
        <button class="guide-close" data-guide="dismiss-invite" aria-label="Dismiss walkthrough invitation">${icon('x')}</button>
        <span class="guide-overline">NEW HERE? START HERE.</span><div class="guide-invite-title"><span>${icon('compass')}</span><h2 id="guideInviteTitle">A little guidance.<br>A clearer first edit.</h2></div>
        <p>Get to know the editor, one useful step at a time. You can pause and pick up later.</p>
        <div class="guide-invite-actions"><button class="primary" data-guide="start">Show me around ${icon('chevronRight')}</button><button class="guide-link" data-guide="dismiss-invite">Not now</button></div>
        <small>Always available from <b>Help</b> · No editing required</small>
      </section>
      <dialog id="guideHub" class="guide-hub" aria-labelledby="guideHubHeading">
        <header class="guide-hub-header"><span>${icon('book')} Help & learning center</span><button class="guide-close" data-guide="close-hub" aria-label="Close Help">${icon('x')}</button></header>
        <div class="guide-hub-hero"><div><span id="guideHubEyebrow" class="guide-overline"></span><h2 id="guideHubHeading" tabindex="-1"></h2><p id="guideHubSub"></p><button id="guideHubAction" class="primary" data-guide="resume"></button><small id="guideResumeDetail"></small></div><div id="guideProgressRing" role="img"><div><b id="guideProgressNumber">0%</b><span id="guideProgressLabel"></span></div></div></div>
        <nav class="guide-hub-tabs" aria-label="Help sections"><button data-guide-hub-tab="tour">Walkthrough</button><button data-guide-hub-tab="answers">Quick answers</button><button data-guide-hub-tab="keys">Shortcuts</button></nav>
        <div id="guideHubBody" class="guide-hub-body"></div>
        <footer class="guide-hub-footer"><details><summary>Progress & preferences <span>Local to this device</span></summary><p id="guidePersistence"></p><label><input type="checkbox" id="guideDimOption"> Dim the surrounding editor during a step</label><div><button class="btn" data-guide="save-progress">${icon('save')}Save progress file</button><button class="btn" data-guide="load-progress">${icon('folder')}Load progress file</button></div><small>Progress files contain lesson identifiers only, not media or project contents.</small></details></footer>
      </dialog>
      <div id="guideLayer" hidden>
        <div id="guideShade" aria-hidden="true"><i></i><i></i><i></i><i></i></div>
        <div id="guideRing" aria-hidden="true"></div><div id="guideTargetLabel" aria-hidden="true"></div>
        <section id="guideCoach" class="guide-coach" role="dialog" aria-modal="false" aria-labelledby="guideCoachTitle" aria-describedby="guideCoachBody">
          <div class="guide-coach-top"><button data-guide="contents" class="guide-contents" title="Open all walkthrough chapters">${icon('book')}<span id="guideChapterLabel"></span></button><div><button class="guide-close" data-guide="minimize" aria-label="Minimize guide">${icon('minus')}</button><button class="guide-close" data-guide="pause" aria-label="Dismiss guide and keep progress">${icon('x')}</button></div></div>
          <div class="guide-coach-copy"><div class="guide-step-meta"><span>GUIDED WALKTHROUGH</span><span id="guideStepCount"></span></div><h2 id="guideCoachTitle" tabindex="-1"></h2><p id="guideCoachBody"></p><div id="guideCoachVisual"></div><div class="guide-tip"><span>${icon('info')}</span><p id="guideCoachTip"></p></div><div id="guideTask" class="guide-task"><span id="guideTaskIcon"></span><p id="guideTaskText"></p></div><p id="guideStatusText" class="guide-step-status" role="status" hidden></p><div class="guide-target-actions"><button id="guideFocus" class="guide-link" data-guide="focus">Focus control</button><span id="guideCoachKeys"></span></div></div>
          <div class="guide-coach-nav"><button class="guide-link" data-guide="pause">Pause guide</button><div><button id="guideBack" class="btn" data-guide="back">Back</button><button id="guideNext" class="primary" data-guide="next">Next</button></div></div>
          <div class="guide-coach-foot"><span id="guideCoachStorage"></span><span>F6 to focus control</span></div><div class="guide-step-progress"><i id="guideProgressLine"></i></div>
        </section>
      </div>
      <div id="guideMini" class="guide-mini" hidden><button data-guide="expand" aria-label="Expand walkthrough">${icon('book')}<span id="guideMiniText"></span>${icon('up')}</button><button data-guide="pause" aria-label="Dismiss minimized guide">${icon('x')}</button></div>
      <dialog id="guideCompletion" class="guide-completion" aria-labelledby="guideCompletionHeading"><button class="guide-close" data-guide="finish-close" aria-label="Close guide summary">${icon('x')}</button><div class="guide-complete-icon">${icon('check')}</div><span class="guide-overline">LEARNING AT YOUR PACE</span><h2 id="guideCompletionHeading" tabindex="-1"></h2><p id="guideCompletionText"></p><div class="guide-project-diagram"><span>${icon('folder')}<b>Save your project</b><small>Keep it editable</small></span><i>+</i><span>${icon('video')}<b>Render your video</b><small>Share its product</small></span></div><div class="guide-complete-actions"><button class="btn" data-guide="finish-lessons">Browse lessons</button><button class="primary" data-guide="finish-close">Back to my edit ${icon('chevronRight')}</button></div></dialog>
      <dialog id="guideRestart" class="guide-reset" aria-labelledby="guideRestartHeading"><h2 id="guideRestartHeading">Start the walkthrough again?</h2><p>This resets only your guide progress. Your project, media, edit history and rendered products stay untouched.</p><div><button id="guideRestartCancel" class="btn" data-guide="cancel-restart">Keep my progress</button><button class="primary" data-guide="confirm-restart">Start over</button></div></dialog>
      <input type="file" id="guideProgressInput" accept=".json,.vbguide.json,application/json" hidden>
      <div id="guideLive" class="guide-sr" role="status" aria-live="polite" aria-atomic="true"></div><div id="guideTargetDescription" class="guide-sr"></div>
    `);
    q('#guideHub').addEventListener('close',()=>{if(!active)focusHelp();});
    q('#guideCompletion').addEventListener('close',()=>focusHelp());
    q('#guideDimOption').addEventListener('change',e=>{state.dim=e.target.checked;persist();scheduleLayout();});
    q('#guideProgressInput').addEventListener('change',e=>{const f=e.target.files?.[0];e.target.value='';importProgress(f);});
    document.addEventListener('input',e=>{if(e.target.id==='guideSearch'){const search=e.target.value.trim().toLowerCase();let hits=0;qa('[data-guide-search]').forEach(n=>{n.hidden=!n.dataset.guideSearch.includes(search);if(!n.hidden)hits++;});q('#guideNoResults').hidden=!!hits;}if(active&&step().action&&e.target.matches(step().action))markTried(step().id);});
    window.addEventListener('click',click,true);
    window.addEventListener('keydown',key,true);
    window.addEventListener('contextmenu',e=>{if(active&&step().context&&e.target.closest('[data-clip],#preview,[data-select-track]'))markTried(step().id);scheduleLayout();},true);
    window.addEventListener('resize',scheduleLayout);
    document.addEventListener('scroll',scheduleLayout,true);
    document.addEventListener('focusin',scheduleLayout,true);
    // Resume the lesson immediately after a native dialog closes; position again next frame.
    document.addEventListener('close',()=>{if(active)layout();scheduleLayout();},true);
    document.addEventListener('visibilitychange',()=>{if(document.hidden&&active)persist();});
    window.addEventListener('pagehide',()=>{if(ready)persist();});
    resizeObserver=new ResizeObserver(scheduleLayout);resizeObserver.observe(q('#guideCoach'));
    appObserver=new MutationObserver(()=>{if(active)scheduleLayout();});appObserver.observe(q('#app'),{childList:true,subtree:true});
    modalObserver=new MutationObserver(mutations=>{if(active&&mutations.some(m=>m.type==='attributes'&&m.target.matches('dialog')||m.type==='childList'&&[...m.addedNodes,...m.removedNodes].some(n=>n.nodeType===1&&(n.matches('.context-panel,dialog')||n.querySelector?.('.context-panel')))))scheduleLayout();});
    modalObserver.observe(document.body,{subtree:true,childList:true,attributes:true,attributeFilter:['open']});
    q('#helpDialog .dialog-body').insertAdjacentHTML('afterbegin',`<div class="guide-legacy-link"><b>Prefer a guided walkthrough?</b><button class="btn" data-guide="hub-from-legacy">Show me around</button></div>`);
    
    const originalRoute=route;route=function(action,el){if(action==='help'){showHub();return;}originalRoute(action,el);};
    ready=true;updateHelp();showInvitation();
    window.VaultBuddyGuide={version:1,key:KEY,start,pause:stop,showHub,next:()=>go(1),back:()=>go(-1),minimize,expand,getState:()=>deep({...state,active,minimized,suspended,storageAvailable:storageOK}),steps:()=>STEPS.map(({id,chapter,title,label})=>({id,chapter,title,label})),parseProgress:v=>parse(deep(v)),exportProgress,importProgress,locate:focusTarget};
  }
  return {init};
})();
