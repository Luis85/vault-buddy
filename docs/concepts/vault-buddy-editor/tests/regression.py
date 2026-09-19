from pathlib import Path
from playwright.sync_api import sync_playwright
import json,traceback,zipfile,subprocess,time,math
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'tests'/'artifacts';OUT.mkdir(exist_ok=True)
HTML=(ROOT/'vault-buddy-editor.html').read_text();RESULTS=[]
def check(name,fn):
 try:
  detail=fn();RESULTS.append({'name':name,'result':'PASS','detail':detail});print('PASS',name,detail or '',flush=True)
 except Exception as e:
  RESULTS.append({'name':name,'result':'FAIL','detail':str(e)});print('FAIL',name,str(e),flush=True);traceback.print_exc()
def assert_(v,msg='Assertion failed'):
 if not v:raise AssertionError(msg)
def reset(page):
 page.evaluate("""()=>{for(const d of document.querySelectorAll('dialog[open]'))d.close();pause();clearRuntime();clearTimeout(saveTimer);project=seed();projectRecord=newProjectRecord(project);history=[];redoStack=[];multiIds=[];clipClipboard=null;closeContext();ui.time=13400;ui.selected={type:'clip',id:'presenter1'};ui.propertyTab='layout';ui.tab='media';ui.zoom=1;ui.snap=true;ui.review=false;productFiles.clear();projectDownloadToken=null;$('#deleteMode').value='gap';prepareBuiltin();render();syncMedia();} """)
def ev(page,js):return page.evaluate(js)
def prop(page,name,value):
 el=page.locator(f'[data-prop="{name}"]');el.evaluate('(el,value)=>{el.value=String(value);el.dispatchEvent(new Event("change",{bubbles:true}));}',value)
def download(page,action,path):
 with page.expect_download(timeout=30000) as d:action()
 d.value.save_as(str(path));return path

# Test setup: dismiss the optional invitation through its actual UI.
# The delivered HTML is loaded byte-for-byte without patching.
from playwright.sync_api import Page
_original_set_content=Page.set_content
def _with_invitation_dismissed(self,*args,**kwargs):
 result=_original_set_content(self,*args,**kwargs)
 self.wait_for_function('!!window.VaultBuddyGuide')
 invitation=self.locator('#guideInvite')
 if invitation.is_visible():
  invitation.locator('[data-guide="dismiss-invite"]').first.click()
 return result
Page.set_content=_with_invitation_dismissed
with sync_playwright() as pw:
 browser=pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True,args=['--no-sandbox','--autoplay-policy=no-user-gesture-required'])
 context=browser.new_context(viewport={'width':1440,'height':960},accept_downloads=True)
 page=context.new_page();page.set_default_timeout(6000);errors=[];network=[];page.on('pageerror',lambda e:errors.append(str(e)));page.on('request',lambda r:network.append(r.url) if r.url.startswith(('http:','https:','ws:','wss:')) else None)
 page.set_content(HTML);page.wait_for_timeout(850)
 def seedcheck():
  v=ev(page,'({valid:!!validate(project),tracks:project.tracks.length,clips:project.clips.length,ms:duration(),camera:webcamState.phase})');assert_(v['valid'] and v['tracks']==5 and v['clips']==7 and v['ms']==33500 and v['camera']=='idle');return v
 check('Sample graph, multiple tracks and camera-off initial state',seedcheck)
 def playback():
  page.locator('#playButton').click();page.wait_for_timeout(700);assert_(ev(page,'ui.playing && ui.time>13700'));page.locator('#playButton').click();assert_(not ev(page,'ui.playing'));ev(page,'seek(11000)');assert_(ev(page,"sourceTime(clip('c2'))===12000"))
 check('Playback, pause and output-to-source mapping',playback)
 def tracks():
  reset(page);page.locator('[data-action="trackMenu"]').click();page.locator('[data-add-track="video"]').click();assert_(ev(page,'project.tracks.length===6 && project.tracks[0].kind==="video"'));ev(page,"addTrack('audio')");assert_(ev(page,'project.tracks.at(-1).kind==="audio"'));ev(page,'undo();undo()');assert_(ev(page,'project.tracks.length===5'))
 check('Add video/audio tracks and undo',tracks)
 def split_test():
  reset(page);old=ev(page,'JSON.stringify(project)');ev(page,"selectClip('c2');seek(14500);split()");assert_(ev(page,'project.clips.length===8 && !!validate(project)'));assert_(ev(page,"project.effects.filter(e=>e.kind==='text'&&effective(e)).length===3"));ev(page,'undo()');assert_(old==ev(page,'JSON.stringify(project)'));ev(page,'redo()');assert_(ev(page,'project.clips.length===8'))
 check('Split preserves intersecting annotations and undo/redo',split_test)
 def reorder():
  reset(page);ev(page,"selectClip('c2');reorderClip(-1)");assert_(ev(page,"clip('c2').start_ms===0 && clip('c1').start_ms===14000 && clip('presenter1').start_ms===1500"));assert_(ev(page,"effective(project.effects.find(e=>e.id==='e-arrow')).output===1000"))
 check('Reorder carries callouts without moving other tracks',reorder)
 def delete_modes():
  reset(page);ev(page,"selectClip('c2');deleteSelection()");assert_(ev(page,"clip('c3').start_ms===23500"));ev(page,'undo()');ev(page,"$('#deleteMode').value='ripple';selectClip('c2');deleteSelection()");assert_(ev(page,"clip('c3').start_ms===9500 && clip('presenter1').start_ms===1500"))
 check('Gap delete versus explicitly track-local ripple delete',delete_modes)
 def locks():
  reset(page);ev(page,"toggleTrack('v3','locked');selectClip('presenter1')");before=ev(page,"JSON.stringify(clip('presenter1'))");ev(page,"moveClip(clip('presenter1'),5000);deleteSelection()");prop(page,'clipScale',25);assert_(before==ev(page,"JSON.stringify(clip('presenter1'))"));ev(page,"toggleTrack('v3','locked')")
 check('Locked tracks reject movement, deletion and layout changes',locks)
 def fades():
  reset(page);page.locator('[data-property-tab="fades"]').click();prop(page,'fadeIn',1.2);prop(page,'fadeOut',.8);assert_(ev(page,"clip('presenter1').fade_in_ms===1200 && clip('presenter1').fade_out_ms===800 && fadeValue(clip('presenter1'),1500)===0"));assert_(abs(ev(page,"fadeValue(clip('presenter1'),2100)")-.5)<.001);prop(page,'fadeIn',999);assert_(ev(page,"clip('presenter1').fade_in_ms===16000"));page.screenshot(path=str(ROOT/'screenshots/05-fades.png'))
 check('Numeric fades, fade envelopes and duration limits',fades)
 def transition():
  reset(page);ev(page,"selectClip('c1');makeCrossfade(1)");assert_(ev(page,"project.transitions.length===1&&clip('c2').start_ms===8500&&clip('c3').start_ms===22500&&clip('music1').start_ms===0"));assert_(ev(page,'!!validate(project)'));ev(page,"removeTransition(project.transitions[0].id)");assert_(ev(page,"clip('c2').start_ms===9500&&project.transitions.length===0"))
 check('Cross dissolve and removal preserve other tracks',transition)
 def audio_transition():
  reset(page);ev(page,"project.clips=project.clips.filter(c=>c.track_id!=='a1');project.clips.push(newClip('toneA','cues','a1','A',0,0,4000),newClip('toneB','cues','a1','B',4000,0,4000));render();selectClip('toneA');makeCrossfade(1)");v=ev(page,"[clipGain(clip('toneA'),3500),clipGain(clip('toneB'),3500)]");assert_(abs(sum(x*x for x in v)-.7**2)<.00001);return v
 check('Equal-power audio crossfade envelope',audio_transition)
 def mixing():
  reset(page);ev(page,"toggleTrack('a1','solo')");assert_(ev(page,"clipGain(clip('music1'),10000)===0 && clipGain(clip('cue1'),10000)>.6"));ev(page,"toggleTrack('a1','muted')");assert_(ev(page,"clipGain(clip('cue1'),10000)===0"));ev(page,"toggleTrack('a1','solo');toggleTrack('a1','muted');ui.monitorMuted=true;syncMedia()");assert_(ev(page,"clipGain(clip('cue1'),10000)>.6"));page.locator('[data-action="mixer"]').click();page.screenshot(path=str(ROOT/'screenshots/06-audio-mixer.png'));page.locator('#mixerDialog [data-action="close"]').click()
 check('Track mute/solo, master mix and independent monitoring mute',mixing)
 def layout():
  reset(page);page.locator('[data-video-preset="pip"]').click();page.locator('.precision-section summary').first.click();page.locator('[data-video-shape="circle"]').click();prop(page,'clipScale',24);page.locator('[data-video-corner="bl"]').click();prop(page,'videoCropZoom',1.5);page.locator('[data-prop="videoMirror"]').check();assert_(ev(page,"Math.abs(selectedClip().w*W-selectedClip().h*H)<1&&selectedClip().x<.05&&selectedClip().mirror&&selectedClip().crop_zoom===1.5&&!!validate(project)"))
 check('Resizable circle/PiP, corner presets, crop and mirror',layout)
 def pointer_resize():
  reset(page);before=ev(page,'deep(selectedClip())');box=page.locator('#preview').bounding_box();x=box['x']+(before['x']+before['w'])*box['width']-2;y=box['y']+(before['y']+before['h'])*box['height']-2
  page.mouse.move(x,y);page.mouse.down();page.mouse.move(x-22,y-22,steps=8);page.mouse.up();after=ev(page,'deep(selectedClip())');assert_(after['w']<before['w']-.02, str(after));assert_(abs(after['w']*1280-after['h']*720)<1);ev(page,'undo()');assert_(ev(page,"Math.abs(selectedClip().w-.19)<.0001"))
 check('Pointer corner resize preserves circle and is one undo',pointer_resize)
 def pointer_move():
  reset(page);before=ev(page,'deep(selectedClip())');box=page.locator('#preview').bounding_box();x=box['x']+(before['x']+before['w']/2)*box['width'];y=box['y']+(before['y']+before['h']/2)*box['height'];page.mouse.move(x,y);page.mouse.down();page.mouse.move(x-55,y+25,steps=8);page.mouse.up();assert_(ev(page,"selectedClip().x<.73 && !!validate(project)"))
 check('Pointer webcam repositioning and bounds',pointer_move)
 def pointer_fade():
  reset(page);h=page.locator('[data-clip="presenter1"] [data-fade="in"]');h.scroll_into_view_if_needed();b=h.bounding_box();page.mouse.move(b['x']+b['width']/2,b['y']+b['height']/2);page.mouse.down();page.mouse.move(b['x']+60,b['y']+b['height']/2,steps=6);page.mouse.up();assert_(ev(page,"clip('presenter1').fade_in_ms>1000&&history.length===1"));ev(page,'undo()');assert_(ev(page,"clip('presenter1').fade_in_ms===600"))
 check('Gold timeline handle changes fade and supports undo',pointer_fade)
 def pointer_trim():
  reset(page);h=page.locator('[data-clip="presenter1"] [data-trim="end"]');h.scroll_into_view_if_needed();b=h.bounding_box();page.mouse.move(b['x']+b['width']/2,b['y']+25);page.mouse.down();page.mouse.move(b['x']-45,b['y']+25,steps=6);page.mouse.up();assert_(ev(page,"clip('presenter1').out_ms<32000&&!!validate(project)"));ev(page,'undo()');assert_(ev(page,"clip('presenter1').out_ms===32000"))
 check('Pointer trim preserves source and reversible edit',pointer_trim)
 def tools():
  reset(page);ev(page,"selectClip('c2');seek(13000)");for_k=['text','arrow','highlight','spotlight','zoom','step'];
  for k in for_k:ev(page,f"addEffect('{k}')")
  assert_(ev(page,'project.effects.length===12 && !!validate(project)'));ev(page,'addMarker()');page.locator('#editText').fill('Demonstrate the next step');page.locator('#editForm button[type="submit"]').click();assert_('Demonstrate the next step' in ev(page,'noteText()'))
 check('Teaching tools, clip-linked markers and generated note',tools)
 def keyboard():
  reset(page);page.locator('#preview').focus();page.keyboard.press('ArrowRight');assert_(ev(page,'ui.time>13420'));page.keyboard.press('n');assert_(not ev(page,'ui.snap'));page.keyboard.press('Control+s');assert_(page.locator('#projectSaveDialog').is_visible());page.keyboard.press('Escape');assert_(not page.locator('#projectSaveDialog').is_visible());page.locator('#resizeBar').focus();old=ev(page,"$('.timeline').getBoundingClientRect().height");page.keyboard.press('ArrowUp');assert_(ev(page,"$('.timeline').getBoundingClientRect().height")!=old)
 check('Keyboard seek, snapping, project save and timeline resize',keyboard)
 def themes():
  reset(page);ev(page,"document.documentElement.dataset.theme='dark';setTimelineHeight(350)");page.wait_for_timeout(250);page.screenshot(path=str(ROOT/'screenshots/01-multitrack-dark.png'));page.locator('#viewMenuButton').click();page.get_by_role('menuitemcheckbox',name='Light theme',exact=True).click();page.wait_for_timeout(250);page.screenshot(path=str(ROOT/'screenshots/02-multitrack-light.png'));assert_(ev(page,"document.documentElement.dataset.theme==='light'"));page.locator('#viewMenuButton').click();page.get_by_role('menuitemcheckbox',name='Light theme',exact=True).click()
 check('Light/dark themes and resized workspace screenshots',themes)
 def media_import():
  reset(page);page.locator('#mediaInput').set_input_files([str(ROOT/'tests/fixtures/capture-with-audio.mp4'),str(ROOT/'tests/fixtures/narration-tone.wav')]);page.wait_for_function("project.assets.filter(a=>!a.builtin).length===2 && !ui.importing",timeout=12000);v=ev(page,"project.assets.filter(a=>!a.builtin).map(a=>({id:a.id,kind:a.kind,duration:a.duration_ms,width:a.width}))");assert_(v[0]['width']==640 and v[1]['kind']=='audio');return v
 check('Real local video/audio import, metadata and peaks',media_import)
 # Preserve imported sources for export tests; build a small real-media composition.
 def prepare_render():
  ev(page,"""()=>{pause();const vids=project.assets.filter(a=>!a.builtin&&a.kind==='video'),aud=project.assets.find(a=>!a.builtin&&a.kind==='audio');project.clips=[newClip('rmain',vids[0].id,'v1','Screen recording',0,0,2600,{fade_in_ms:300,fade_out_ms:300}),newClip('rpip',vids[0].id,'v3','Presenter overlay',200,0,2400,{x:.72,y:.10,w:.20,h:.20*W/H,frame_shape:'circle',fit:'cover',muted:true,fade_in_ms:300,fade_out_ms:300}),newClip('rsound',aud.id,'a1','Narration test tone',0,0,2600,{volume:.25,fade_in_ms:200,fade_out_ms:400,fade_curve:'equal-power'})];project.tracks=project.tracks.filter(t=>['v3','v1','a1'].includes(t.id));project.tracks.forEach(t=>{t.muted=t.solo=false;t.visible=true;t.volume=1});project.master_gain=.8;project.effects=[{id:'rtext',clip_id:'rmain',kind:'text',start_ms:0,end_ms:2600,x:.20,y:.80,w:.65,h:.08,color:'#ffffff',background:true,fontSize:32,text:'Two video layers. Two mixed audio sources.'}];project.markers=[];project.transitions=[];project.title='Workspace roundtrip test';project.id='verified-workspace';projectRecord=newProjectRecord(project);history=[];redoStack=[];ui.time=1000;ui.selected={type:'clip',id:'rpip'};ui.tab='project';ui.propertyTab='layout';ui.zoom=2;setTimelineHeight(335);validate(project);render();syncMedia(true);} """)
  page.wait_for_timeout(700);assert_(ev(page,"projectHealth().missing.length===0 && runtime(clip('rmain')).el.readyState>=2"));page.locator('#playButton').click();page.wait_for_timeout(500);assert_(ev(page,"[...runtimes.values()].filter(r=>r.gain&&r.gain.gain.value>0.05).length>=2"));page.locator('#playButton').click();ev(page,'seek(1000)')
 check('Independent real video layers and simultaneous audio playback',prepare_render)
 before_zip=OUT/'before-render.vbproject.zip'
 def save_without_render():
  download(page,lambda:(page.locator('.save-project-btn').click(),page.locator('[data-action="confirmProjectSave"]').click()),before_zip)
  z=zipfile.ZipFile(before_zip);assert_(z.testzip() is None);w=json.loads(z.read('project.vbproject.json'));assert_(w['record']['products']==[] and len(w['project']['clips'])==3 and w['workspace']['timeline_zoom']==2 and not any(n.startswith('Products/') for n in z.namelist()));assert_(len(json.loads(z.read('manifest.json'))['source_files'])==2);page.locator('#projectSaveDialog [data-action="close"]').first.click();return z.namelist()
 check('Save portable workspace without rendering: sources, layout, CRC',save_without_render)
 def fresh_open():
  c2=browser.new_context(viewport={'width':1440,'height':960},accept_downloads=True);p2=c2.new_page();p2.set_content(HTML);p2.wait_for_timeout(700);p2.locator('#projectInput').set_input_files(str(before_zip));p2.wait_for_selector('#confirmDialog[open]');p2.locator('#confirmButton').click();p2.wait_for_function("project.id==='verified-workspace' && !projectBusy");assert_(ev(p2,"projectHealth().missing.length===0 && ui.zoom===2 && ui.propertyTab==='layout' && projectRecord.products.length===0"));p2.wait_for_timeout(700);assert_(ev(p2,"runtime(clip('rmain')).el.readyState>=2"));p2.screenshot(path=str(ROOT/'screenshots/07-restored-workspace.png'));c2.close()
 check('Reopen portable project in a fresh context with playable sources',fresh_open)
 rendered_zip=OUT/'rendered-product.zip'
 def render_test():
  download(page,lambda:(page.locator('.header-actions [data-action="save"]').click(),page.locator('[data-action="downloadPackage"]').click()),rendered_zip)
  z=zipfile.ZipFile(rendered_zip);assert_(z.testzip() is None);names=z.namelist();video=next(n for n in names if n.endswith(('.webm','.mp4')));(OUT/'rendered-review.webm').write_bytes(z.read(video));pinfo=json.loads(z.read('Render/product.json'));assert_(pinfo['project_id']=='verified-workspace' and pinfo['revision']==1 and len(pinfo['snapshot']['clips'])==3);assert_(ev(page,'projectRecord.products.length===1 && !ui.exporting'));page.locator('[data-action="showProducts"]').click();page.screenshot(path=str(ROOT/'screenshots/08-project-products.png'));return {'files':names,'product_id':pinfo['id']}
 check('Real multi-track render creates an immutable product and editable snapshot',render_test)
 def probe_render():
  raw=subprocess.run(['ffprobe','-v','error','-show_entries','stream=codec_name,codec_type,width,height','-of','json',str(OUT/'rendered-review.webm')],capture_output=True,text=True,check=True).stdout;info=json.loads(raw);assert_(any(s['codec_type']=='video' and s['width']==1280 for s in info['streams']) and any(s['codec_type']=='audio' for s in info['streams']));pcm=subprocess.run(['ffmpeg','-v','error','-i',str(OUT/'rendered-review.webm'),'-vn','-ac','1','-ar','48000','-f','f32le','-'],capture_output=True,check=True).stdout
  import numpy as np
  x=np.frombuffer(pcm,dtype=np.float32);assert_(np.sqrt(np.mean(x*x))>.01);spectrum=np.abs(np.fft.rfft(x));freq=np.fft.rfftfreq(len(x),1/48000);peaks={f:float(spectrum[np.abs(freq-f).argmin()]) for f in [440,880]};assert_(min(peaks.values())>2);return {'streams':info['streams'],'audio_rms':float(np.sqrt(np.mean(x*x))),'tone_magnitudes':peaks}
 check('Rendered streams decode with both mixed audio frequencies',probe_render)
 def product_independence():
  snap=ev(page,'JSON.stringify(projectRecord.products[0].snapshot)');ev(page,"selectClip('rpip');change(()=>clip('rpip').opacity=.6,'Refine webcam opacity');ui.tab='project';render()");assert_(ev(page,'projectRecord.revision>1 && projectRecord.products.length===1'));assert_(snap==ev(page,'JSON.stringify(projectRecord.products[0].snapshot)'));assert_('Earlier edit' in page.locator('#library').inner_text() or 'earlier' in page.locator('#library').inner_text().lower());ev(page,'restoreRenderedRevision(projectRecord.products[0].id)');page.locator('#confirmButton').click();assert_(ev(page,'editFingerprint()===projectRecord.products[0].edit_fingerprint && projectRecord.products.length===1'));ev(page,'undo()');assert_(ev(page,"clip('rpip').opacity===.6"))
 check('Edit after rendering, restore its revision and undo without mutating output',product_independence)
 after_zip=OUT/'after-render.vbproject.zip'
 def save_products():
  download(page,lambda:(page.locator('.save-project-btn').click(),page.locator('[data-action="confirmProjectSave"]').click()),after_zip);z=zipfile.ZipFile(after_zip);assert_(z.testzip() is None);w=json.loads(z.read('project.vbproject.json'));assert_(len(w['record']['products'])==1 and any(n.startswith('Products/') for n in z.namelist()));page.locator('#projectSaveDialog [data-action="close"]').first.click()
 check('Project package includes render history, sources and product video',save_products)
 def reopen_products():
  c=browser.new_context(accept_downloads=True);p=c.new_page();p.set_content(HTML);p.wait_for_timeout(500);p.locator('#projectInput').set_input_files(str(after_zip));p.wait_for_selector('#confirmDialog[open]');p.locator('#confirmButton').click();p.wait_for_function("project.id==='verified-workspace'&&!projectBusy");assert_(ev(p,'projectRecord.products.length===1 && productFiles.size===1 && projectHealth().missing.length===0'));c.close()
 check('Project with prior renders reopens in another fresh context',reopen_products)
 def retained_history_sources():
  c=browser.new_context(accept_downloads=True);p=c.new_page();p.set_content(HTML);p.wait_for_timeout(300);p.locator('#projectInput').set_input_files(str(after_zip));p.wait_for_selector('#confirmDialog[open]');p.locator('#confirmButton').click();p.wait_for_function("project.id==='verified-workspace'&&!projectBusy")
  ev(p,"change(()=>{const audio=new Set(project.assets.filter(a=>a.kind==='audio').map(a=>a.id));project.clips=project.clips.filter(c=>!audio.has(c.asset_id));project.assets=project.assets.filter(a=>!audio.has(a.id));},'Remove audio from current edit')")
  f=OUT/'retained-history.vbproject.zip';download(p,lambda:(p.locator('.save-project-btn').click(),p.locator('[data-action="confirmProjectSave"]').click()),f)
  z=zipfile.ZipFile(f);w=json.loads(z.read('project.vbproject.json'));assert_(len([a for a in w['project']['assets'] if not a.get('builtin')])==1 and len(json.loads(z.read('manifest.json'))['source_files'])==2)
  c2=browser.new_context(accept_downloads=True);q=c2.new_page();q.set_content(HTML);q.wait_for_timeout(300);q.locator('#projectInput').set_input_files(str(f));q.wait_for_selector('#confirmDialog[open]');q.locator('#confirmButton').click();q.wait_for_function("project.id==='verified-workspace'&&!projectBusy")
  ev(q,'restoreRenderedRevision(projectRecord.products[0].id)');q.locator('#confirmButton').click();q.wait_for_timeout(300);assert_(ev(q,"project.clips.length===3&&projectHealth().missing.length===0&&project.assets.filter(a=>a.kind==='audio').every(a=>media.has(a.id))"));c2.close();c.close()
 check('Portable project retains source files needed only by a prior rendered revision',retained_history_sources)
 def missing_sources():
  c=browser.new_context(accept_downloads=True);p=c.new_page();p.set_content(HTML);p.wait_for_timeout(500);w=json.loads(zipfile.ZipFile(after_zip).read('project.vbproject.json'));file=OUT/'reference.vbproject.json';file.write_text(json.dumps(w));p.locator('#projectInput').set_input_files(str(file));p.wait_for_selector('#confirmDialog[open]');p.locator('#confirmButton').click();p.wait_for_function("project.id==='verified-workspace'&&!projectBusy");assert_(ev(p,'projectHealth().missing.length===2'));ev(p,'openSave()');assert_(p.locator('[data-action="downloadPackage"]').is_disabled());p.locator('#saveDialog [data-action="saveProjectFromRender"]').click();assert_(p.locator('[data-action="confirmProjectSave"]').is_enabled());c.close()
 check('Lightweight project missing-media state allows saving but blocks rendering',missing_sources)
 def malicious():
  reset(page);errors=ev(page,"""()=>{const cases=[];for(const mutate of [p=>p.assets[0].url='https://bad.invalid/video',p=>p.clips[0].x=NaN,p=>p.clips[0].frame_shape='evil',p=>p.clips[0].crop_zoom=100,p=>p.clips[0].id='../bad',p=>p.destination.folder='../Vault',p=>p.clips[0].track_id='a1']){let p=seed();mutate(p);try{validate(p);cases.push(false)}catch{cases.push(true)}}return cases;}""");assert_(all(errors));return len(errors)
 check('Reject malformed graphs, unsafe paths, remote media and invalid crop',malicious)
 def zip_tamper():
  data=bytearray(before_zip.read_bytes());data[200]^=1;file=OUT/'corrupt.vbproject.zip';file.write_bytes(data);page.locator('#projectInput').set_input_files(str(file));page.wait_for_timeout(400);assert_('integrity' in page.locator('#toast').inner_text().lower());assert_(ev(page,"project.id==='tutorial-multitrack-demo'"))
 check('Corrupt ZIP integrity rejection leaves workspace untouched',zip_tamper)
 def camera_states():
  reset(page);page.locator('.webcam-entry').click();assert_(ev(page,"webcamState.phase==='idle' && !webcamState.stream"));page.locator('[data-cam="enable"]').click();assert_('secure' in page.locator('#webcamMessage').inner_text().lower());page.locator('[data-cam="close"]').first.click();page.evaluate((ROOT/'tests/camera-fixture.js').read_text());page.locator('.webcam-entry').click();ev(page,"()=>{window.__originalGum=navigator.mediaDevices.getUserMedia;navigator.mediaDevices.getUserMedia=async()=>{throw new DOMException('Denied','NotAllowedError')};}");page.locator('[data-cam="enable"]').click();page.wait_for_timeout(300);assert_('denied' in page.locator('#webcamMessage').inner_text().lower());ev(page,'()=>{navigator.mediaDevices.getUserMedia=window.__originalGum;}');page.locator('[data-cam="enable"]').click();page.wait_for_function("webcamState.phase==='preview'");assert_(ev(page,"webcamState.stream.getAudioTracks().length===0"));page.locator('[data-cam="record"]').click();page.wait_for_function("webcamState.phase==='countdown'");page.locator('[data-cam="cancelCountdown"]').click();assert_(ev(page,"webcamState.phase==='preview'&&!webcamState.recorder"));page.locator('[data-cam="close"]').first.click();assert_(ev(page,"webcamState.phase==='idle'&&window.__testTracks.every(t=>t.readyState==='ended')"))
 check('Camera secure-context/denial UI, optional mic, countdown cancel and release (simulated devices)',camera_states)
 def delayed_camera():
  ev(page,"()=>{navigator.mediaDevices.getUserMedia=async c=>{const s=await window.__originalGum(c);await new Promise(r=>setTimeout(r,700));return s};}");page.locator('.webcam-entry').click();page.locator('[data-cam="enable"]').click();page.wait_for_timeout(80);page.locator('[data-cam="close"]').first.click();page.wait_for_timeout(1000);assert_(ev(page,"webcamState.phase==='idle'&&window.__testTracks.every(t=>t.readyState==='ended')"));ev(page,'()=>{navigator.mediaDevices.getUserMedia=window.__originalGum;}')
 check('Late camera grant after close releases all tracks (simulated permission)',delayed_camera)
 def responsive():
  reset(page);details=[]
  for w,h in [(1600,1000),(1280,800),(960,640),(720,480),(390,844)]:
   page.set_viewport_size({'width':w,'height':h});page.wait_for_timeout(200)
   v=ev(page,'({body:document.body.scrollWidth,view:innerWidth,render:document.querySelector(".header-actions [data-action=save]").getBoundingClientRect().width>0})');assert_(v['body']<=w+1 and v['render'],str(v))
   # reference intentionally overflows less frequent tools rather than keeping every icon visible.
   # Inspect the actual menu, requiring each teaching capability to remain reachable.
   pinned=page.locator('.toolstrip [data-effect]:visible').evaluate_all('(es)=>es.map(e=>e.dataset.effect)')
   page.locator('#moreToolsButton').click();labels=page.locator('#focusCommandMenu [role=menuitem]').all_text_contents()
   for key,label in [('text','Text'),('arrow','Arrow'),('highlight','Highlight'),('zoom','Zoom'),('spotlight','Spotlight'),('step','Numbered step'),('mask','Privacy cover')]:
    assert_(key in pinned or any(t.strip()==label for t in labels),f'{w}px: missing {label}')
   page.keyboard.press('Escape');details.append([w,h]);page.screenshot(path=str(ROOT/'screenshots'/f'compact-{w}.png'))
  page.set_viewport_size({'width':1440,'height':960});return details
 check('Responsive containment at five viewport sizes',responsive)
 def stress():
  reset(page);v=ev(page,"""()=>{const s=performance.now();project.effects=[];project.markers=[];project.transitions=[];project.clips=[];for(let i=0;i<100;i++)project.clips.push(newClip('stress'+i,'screen','v1','Segment '+i,i*500,0,500));ui.time=5000;ui.selected=null;validate(project);render();return {clips:100,render_ms:performance.now()-s};}""");assert_(v['render_ms']<1500);reset(page);return v
 check('100-clip timeline rendering smoke measurement',stress)
 check('No uncaught JavaScript errors or network requests',lambda:(assert_(not errors,str(errors)),assert_(not network,str(network))))
 # Raw-frame export verifies canvas remains untainted and produces real PNG bytes.
 def png():
  file=OUT/'annotated-frame.png';page.locator('#viewMenuButton').click();download(page,lambda:page.get_by_role('menuitem',name='Download annotated frame…',exact=True).click(),file);assert_(file.read_bytes().startswith(b'\x89PNG'));return file.stat().st_size
 check('Annotated PNG export',png)
 browser.close()
(ROOT/'tests/results.json').write_text(json.dumps(RESULTS,indent=2))
print('TOTAL',len(RESULTS),'PASS',sum(r['result']=='PASS' for r in RESULTS),'FAIL',sum(r['result']=='FAIL' for r in RESULTS))
