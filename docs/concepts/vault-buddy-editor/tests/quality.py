"""Review-driven safety regression scenarios. Real DOM/media/downloads; browser origin persistence excluded."""
from pathlib import Path
from playwright.sync_api import sync_playwright
import json,traceback,hashlib,base64,zipfile,io,time
ROOT=Path(__file__).resolve().parents[1]; HTML=(ROOT/'vault-buddy-editor.html').read_text();OUT=ROOT/'tests/artifacts';OUT.mkdir(exist_ok=True)
results=[];errors=[];requests=[];downloads=[]
def expect(v,msg='Assertion failed'):
 if not v:raise AssertionError(msg)
def ev(s):return page.evaluate(s)
def reset():
 ev("""()=>{VaultBuddyGuide.pause(false);for(const d of document.querySelectorAll('dialog[open]'))d.close();pause();closeContext();clearTimeout(saveTimer);Quality.releaseSessionMedia();project=seed();projectRecord=newProjectRecord(project);projectDownloadToken=null;history=[];redoStack=[];multiIds=[];ui.time=13400;ui.selected={type:'clip',id:'presenter1'};ui.propertyTab='layout';ui.tab='media';ui.zoom=1;ui.review=false;ui.mediaSearch='';ui.importing=false;projectBusy=false;webcamState.take=null;webcamState.phase='idle';document.documentElement.dataset.theme='dark';$('#workspace').classList.remove('inspector-open','library-hidden');$('#app').classList.remove('library-drawer');prepareBuiltin();render();syncMedia();} """)
 page.wait_for_timeout(30)
def check(name,fn):
 try:
  reset();d=fn();results.append({'name':name,'status':'PASS','detail':d});print('PASS',name,d or '',flush=True)
 except Exception as e:
  results.append({'name':name,'status':'FAIL','detail':str(e)});print('FAIL',name,str(e),flush=True);traceback.print_exc()
  try:page.screenshot(path=str(OUT/('quality-fail-'+str(len(results))+'.png')))
  except:pass
with sync_playwright() as pw:
 b=pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True,args=['--no-sandbox','--autoplay-policy=no-user-gesture-required'])
 ctx=b.new_context(viewport={'width':1600,'height':1000},accept_downloads=True);page=ctx.new_page();page.set_default_timeout(6000)
 page.on('pageerror',lambda e:errors.append(str(e)));page.on('request',lambda r:requests.append(r.url) if r.url.startswith(('http:','https:','ws:','wss:')) else None);page.on('download',lambda d:downloads.append(d.suggested_filename))
 page.set_content(HTML);page.wait_for_function('!!window.VaultBuddyQuality');page.locator('#guideInvite [data-guide=dismiss-invite]').first.click()
 def clip_focus():
  t=ev('ui.time');a=ev("clip('presenter1').start_ms");page.locator('.clip[data-clip=presenter1]').focus();page.keyboard.press('ArrowRight');page.keyboard.press('ArrowRight')
  expect(ev("clip('presenter1').start_ms")==a+66,'Two keys must move the clip twice');expect(ev('ui.time')==t,'Clip movement must not seek');expect(ev('document.activeElement.dataset.clip')=='presenter1');page.keyboard.press('ArrowLeft');expect(ev('document.activeElement.dataset.clip')=='presenter1');return {'clip_delta_ms':ev("clip('presenter1').start_ms")-a,'playhead_unchanged':True}
 check('Repeated clip nudges retain keyboard focus and never become playhead seeks',clip_focus)
 def track_focus():
  q='.track-controls [data-toggle-track=v3][data-key=muted]';page.locator(q).focus();page.keyboard.press('Enter');expect(ev('document.activeElement.dataset.toggleTrack')=='v3');before=ev("track('v3').muted");page.keyboard.press('Enter');expect(ev("track('v3').muted")!=before);expect(page.locator(q).get_attribute('aria-pressed')==str(ev("track('v3').muted")).lower())
 check('Track toggle keyboard focus and aria-pressed survive rerendering',track_focus)
 def ruler():
  page.locator('#ruler').focus();t=ev('ui.time');page.keyboard.press('PageUp');expect(abs(ev('ui.time')-t-1000)<1);page.keyboard.press('PageDown');expect(abs(ev('ui.time')-t)<1);page.keyboard.press('ArrowUp');expect(abs(ev('ui.time')-t-1000/30)<1);page.keyboard.press('ArrowDown');expect(abs(ev('ui.time')-t)<1)
 check('Timeline slider supports Page and vertical-arrow keyboard alternatives',ruler)
 def unavailable_split():
  ev("project.tracks.find(t=>t.id==='v3').locked=true;render()");expect(page.locator('#splitButton').is_disabled());expect('Unlock' in page.locator('#splitButton').get_attribute('title'))
  ev("track('v3').locked=false;seek(clip('presenter1').start_ms);render()");expect(page.locator('#splitButton').is_disabled());expect('playhead' in page.locator('#splitButton').get_attribute('title'));ev('seek(5000);render()');expect(not page.locator('#splitButton').is_disabled())
 check('Split explains locked-track and boundary prerequisites before activation',unavailable_split)
 def save_radios():
  page.locator('.header-actions [data-action=downloadProject]').click();q=page.locator('input[name=projectFormat]').first;r=q.bounding_box();expect(r['width']==18 and r['height']==18,str(r));page.locator('.save-option').nth(1).click();expect(ev("document.querySelector('input[name=projectFormat]:checked').value")=='reference');q.focus();page.keyboard.press('Space');expect(q.is_checked());return r
 check('Project format radios have native-size targets inside full clickable labels',save_radios)
 def modal_fit():
  values=[]
  for w,h in [(1600,1000),(960,640),(720,640),(390,844)]:
   page.set_viewport_size({'width':w,'height':h});ev('downloadProject()');page.wait_for_timeout(50)
   r=page.locator('#projectSaveDialog').bounding_box();f=page.locator('#projectSaveFooter').bounding_box();expect(r['x']>=0 and r['y']>=0 and r['x']+r['width']<=w+1 and f['y']+f['height']<=h+1,str((w,r,f)));expect(ev("getComputedStyle(document.querySelector('#projectSaveBody')).overflowY")=='auto');values.append({'width':w,'footer_bottom':f['y']+f['height']});page.keyboard.press('Escape')
  page.set_viewport_size({'width':1600,'height':1000});return values
 check('Save dialogs keep their footer in view across four window sizes',modal_fit)
 def compact():
  page.set_viewport_size({'width':960,'height':640});page.wait_for_timeout(100);r=page.locator('#canvasWrap').bounding_box();expect(r['width']>=270 and r['height']>=150,str(r));expect(not page.locator('#selectionBar').is_visible());page.locator('.clip[data-clip=presenter1]').click(button='right');expect(page.locator('[role=menu]').is_visible());page.keyboard.press('Escape');page.set_viewport_size({'width':1600,'height':1000});return r
 check('Compact workspace enlarges the preview without removing contextual commands',compact)
 def targets():
  dims=page.locator('.track-controls button').evaluate_all('es=>es.map(e=>({w:e.getBoundingClientRect().width,h:e.getBoundingClientRect().height}))');expect(all(d['w']>=24 and d['h']>=24 for d in dims),str(dims));page.emulate_media(forced_colors='active');page.locator('.clip[data-clip=presenter1]').focus();expect(ev("getComputedStyle(document.activeElement).outlineStyle")!='none');page.emulate_media(forced_colors='none');return {'track_controls':len(dims),'minimum_width':min(d['w'] for d in dims),'minimum_height':min(d['h'] for d in dims)}
 check('Track targets are at least 24 pixels and forced-colors focus is visible',targets)
 def status():
  before=ev('JSON.stringify(project)');page.locator('#saveStatus').focus();page.keyboard.press('Enter');expect(page.locator('#sessionDialog').is_visible());text=page.locator('#sessionBody').inner_text();expect('uncensored' in text and 'Session only' in text and 'not a' in text.lower());page.keyboard.press('Escape');page.wait_for_function("document.activeElement.id==='saveStatus'");expect(before==ev('JSON.stringify(project)'));page.locator('#storageInfo').focus();page.keyboard.press('Space');expect(page.locator('#sessionDialog').is_visible())
 check('Keyboard-accessible project status explains files and restores focus without an edit',status)
 def invalid_links():
  d=ev("""()=>{const cases=['cycle','self','missing','builtin','chain'];return cases.map(kind=>{const p=seed();p.assets.push({id:'root-original',kind:'video',name:'Original',duration_ms:1000});const a={id:'child-a',kind:'audio',name:'A',duration_ms:1000,linked_asset:'root-original'};p.assets.push(a);if(kind==='cycle'){a.linked_asset='child-b';p.assets.push({...a,id:'child-b',linked_asset:'child-a'});}if(kind==='self')a.linked_asset=a.id;if(kind==='missing')a.linked_asset='not-here';if(kind==='builtin')a.linked_asset='screen';if(kind==='chain'){a.linked_asset='child-b';p.assets.push({...a,id:'child-b',linked_asset:'root-original'});}try{validate(p);return {kind,rejected:false}}catch(e){return {kind,rejected:true,message:e.message}}});}""");expect(all(x['rejected'] for x in d),str(d));return d
 check('Project validation rejects cyclic, self, missing, builtin and chained audio aliases',invalid_links)
 def good_link():
  expect(ev("""()=>{const p=seed();p.assets.push({id:'root-original',kind:'video',name:'Original',duration_ms:1000,width:1920,height:1080,size:500,last_modified:123},{id:'child-a',kind:'audio',name:'Detached',duration_ms:1000,linked_asset:'root-original'});return !!validate(p)}"""))
 check('Valid direct detached-audio aliases remain compatible with saved projects',good_link)
 def invalid_meta():
  d=ev("""()=>{return [['width',-1],['height',Infinity],['width',3.5],['height',20000],['size',-3],['size',Infinity],['last_modified',-1]].map(([k,v])=>{const p=seed();p.assets[0][k]=v;try{validate(p);return false}catch{return true}})}""");expect(all(d),str(d));return len(d)
 check('Source dimension, byte-size and timestamp metadata reject invalid numeric values',invalid_meta)
 def source_release():
  ev("window.__revoked=[];window.__oldRevoke=URL.revokeObjectURL;URL.revokeObjectURL=u=>{__revoked.push(u);__oldRevoke(u)};window.__url=URL.createObjectURL(new Blob(['media']));media.set('unused-original',{file:new Blob(['media']),url:__url});newEmptyProject()")
  page.locator('#confirmDialog [data-action=close]').click();expect(ev("media.has('unused-original')"));expect(ev('__revoked.length')==0);ev('newEmptyProject()');page.locator('#confirmDialog .primary').click();expect(ev('media.size')==0);expect(ev('__revoked.includes(__url)'));ev('URL.revokeObjectURL=__oldRevoke');return 'Cancelled replacement retains media; confirmed replacement releases it.'
 check('Source URLs are released only after confirmed workspace replacement',source_release)
 def undo_retains():
  ev("media.set('history-source',{url:URL.createObjectURL(new Blob(['x'])),file:new Blob(['x'])});change(()=>project.title='Changed title','Rename');undo()");expect(ev("media.has('history-source')"))
 check('Ordinary editing and undo do not evict source media',undo_retains)
 def unsafe_close():
  v=ev("""()=>{projectDownloadToken=documentToken();const states=[];for(const phase of ['idle','countdown','recording','stopping']){webcamState.phase=phase;webcamState.take=null;const event=new Event('beforeunload',{cancelable:true});window.dispatchEvent(event);states.push({phase,prevented:event.defaultPrevented});}webcamState.phase='review';webcamState.take={};const event=new Event('beforeunload',{cancelable:true});window.dispatchEvent(event);states.push({phase:'review',prevented:event.defaultPrevented});webcamState.phase='idle';webcamState.take=null;return states;}""");expect(not v[0]['prevented'] and all(x['prevented'] for x in v[1:]),str(v));return v
 check('Close guard includes recording/countdown and uncommitted takes (synthetic event)',unsafe_close)
 def cache():
  before=ev('VaultBuddyQuality.getStats()');ev('for(let i=0;i<12;i++)collectIssues()');after=ev('VaultBuddyQuality.getStats()');expect(after['issueHits']>=before['issueHits']+11);ev("project.captions=captionDefaults();project.captions.cues=[{id:'q1',clip_id:'screen1',start_ms:0,end_ms:1000,text:'Caption'}]")
  # Bind to a real clip and distinguish content changes, not just revisions.
  ev("project.captions.cues[0].clip_id=project.clips[0].id;project.captions.enabled=false");expect(ev("collectIssues().some(i=>i.id==='captions-excluded')"));ev('project.captions.enabled=true;project.captions.burn_in=true');expect(not ev("collectIssues().some(i=>i.id==='captions-excluded')"));return {'cache_hits_added':after['issueHits']-before['issueHits']}
 check('Checks cache reuses unchanged results and invalidates on graph changes',cache)
 def take_issue():
  expect(not ev("collectIssues().some(i=>i.id==='uncommitted-take')"));ev("webcamState.phase='review';webcamState.take={}");expect(ev("collectIssues().some(i=>i.id==='uncommitted-take')"));ev("webcamState.phase='idle';webcamState.take=null");expect(not ev("collectIssues().some(i=>i.id==='uncommitted-take')"))
 check('Uncommitted camera warning appears and clears without requiring a project edit',take_issue)
 def transparent():
  ev("project.clips.filter(c=>track(c.track_id).kind==='video').forEach(c=>c.opacity=0);render();openSave()");expect(ev("collectIssues().some(i=>i.id==='transparent-video'&&i.severity==='warning')"));expect(not page.locator('#saveFooter [data-action=downloadPackage]').is_disabled());expect(ev("collectIssues().find(i=>i.id==='transparent-video').detail.includes('intentional')"))
 check('Transparent footage is warned about without blocking intentional overlay-only output',transparent)
 def no_new_error_from_warning():
  ev("project.captions=captionDefaults();project.captions.cues=[{id:'q1',clip_id:project.clips[0].id,start_ms:0,end_ms:1000,text:'Caption'}];project.captions.burn_in=false;render();openSave()");expect(not page.locator('#saveFooter [data-action=downloadPackage]').is_disabled());expect(ev("collectIssues().some(i=>i.id==='captions-excluded'&&i.severity==='warning')"))
 check('Caption exclusion is actionable but does not falsely prohibit an intentional render',no_new_error_from_warning)
 def cancel_save():
  ev("window.__realBuild=buildProjectPackage;buildProjectPackage=async(...a)=>{await new Promise(r=>window.__allowBuild=r);return __realBuild(...a)};downloadProject()");n=len(downloads);token=ev('projectDownloadToken');page.locator('[data-action=confirmProjectSave]').click();page.wait_for_function('!!window.__allowBuild');expect(page.locator('[data-quality=cancel-save]').is_visible());page.locator('[data-quality=cancel-save]').click();ev('__allowBuild()');page.wait_for_function('!projectBusy');expect(len(downloads)==n);expect(ev('projectDownloadToken')==token);expect(not page.locator('#projectSaveDialog').is_visible());ev('buildProjectPackage=__realBuild;delete window.__allowBuild')
 check('Cancel project preparation produces no download and does not mark edits saved',cancel_save)
 def escape_save():
  ev("window.__realBuild=buildProjectPackage;buildProjectPackage=async(...a)=>{await new Promise(r=>window.__allowBuild=r);return __realBuild(...a)};downloadProject()");n=len(downloads);page.locator('[data-action=confirmProjectSave]').click();page.wait_for_function('!!window.__allowBuild');page.keyboard.press('Escape');expect(ev('Quality.saveJob.cancelled'));ev('__allowBuild()');page.wait_for_function('!projectBusy');expect(len(downloads)==n);ev('buildProjectPackage=__realBuild;delete window.__allowBuild')
 check('Escape while preparing a project cancels rather than triggering a later download',escape_save)
 def real_save():
  ev('downloadProject()')
  with page.expect_download() as di:page.locator('[data-action=confirmProjectSave]').click()
  dl=di.value;dest=OUT/'quality-project.vbproject.zip';dl.save_as(str(dest));z=zipfile.ZipFile(dest);expect(z.testzip() is None);expect('project.vbproject.json' in z.namelist());page.wait_for_function('!projectBusy');expect(not ev('VaultBuddyQuality.inspectSession().dirty'));expect('Project download started' in page.locator('#projectSaveTitle').inner_text());return {'size':dest.stat().st_size,'entries':len(z.namelist())}
 check('Actual portable project ZIP downloads with a valid edit graph and honest status',real_save)
 def failure_save():
  ev("window.__realDownload=downloadBlob;downloadBlob=()=>{throw new Error('TEST DOWNLOAD FAILURE')};downloadProject()");page.locator('[data-action=confirmProjectSave]').click();page.wait_for_function('!projectBusy');expect(ev('VaultBuddyQuality.inspectSession().dirty'));expect('TEST DOWNLOAD FAILURE' in page.locator('#projectSaveError').inner_text());expect(page.locator('[data-action=confirmProjectSave]').is_enabled());ev('downloadBlob=__realDownload;void 0')
 check('A rejected download stays dirty and offers retry without losing the workspace',failure_save)
 def mixed_import():
  page.locator('#mediaInput').set_input_files([{'name':'notes.txt','mimeType':'text/plain','buffer':b'not video'},{'name':'local-still.png','mimeType':'image/png','buffer':(ROOT/'tests/fixtures/local-still.png').read_bytes()}]);page.wait_for_function('!ui.importing&&VaultBuddyQuality.getImportReport().length===2');r=ev('VaultBuddyQuality.getImportReport()');expect(r[0]['status']=='skipped' and r[1]['status']=='added',str(r));expect(page.locator('.import-summary').is_visible());page.locator('[data-quality=import-report]').click();expect(page.locator('.import-result').count()==2);expect('notes.txt' in page.locator('#importReportBody').inner_text());return r
 check('Mixed successful and unsupported imports retain an actionable result for each file',mixed_import)
 def import_cancel():
  ev("window.__realInspect=inspectFile;window.__n=0;inspectFile=async f=>{__n++;if(__n===2)await new Promise(r=>window.__allowInspect=r);return __realInspect(f)};void 0")
  buf=(ROOT/'tests/fixtures/local-still.png').read_bytes();page.locator('#mediaInput').set_input_files([{'name':n+'.png','mimeType':'image/png','buffer':buf} for n in ['first','second','third']]);page.wait_for_function('!!window.__allowInspect');expect(ev("VaultBuddyQuality.getImportReport()[0].status")=='added');page.locator('[data-quality=cancel-import]').click();ev('__allowInspect()');page.wait_for_function('!ui.importing');r=ev('VaultBuddyQuality.getImportReport()');expect([x['status'] for x in r]==['added','cancelled','cancelled'],str(r));expect(ev("project.assets.filter(a=>['first.png','second.png','third.png'].includes(a.name)).length")==1);ev('inspectFile=__realInspect;delete window.__allowInspect');return [x['status'] for x in r]
 check('Stopping a media batch retains successful files and reports unprocessed items',import_cancel)
 def import_replace_guard():
  ev("ui.importing=true;newEmptyProject()");expect(page.locator('#confirmDialog').is_hidden());expect('import' in page.locator('#toast').inner_text());ev('resetDemo()');expect(page.locator('#confirmDialog').is_hidden());before=ev('project.id');ev("openProjectFile(new File(['{}'],'empty.json'));ui.importing=false");expect(ev('project.id')==before)
 check('New, reset and project-open operations cannot race an active media import',import_replace_guard)
 def import_cap():
  ev("while(project.assets.length<200)project.assets.push({id:'cap'+project.assets.length,name:'External',kind:'audio',duration_ms:1000});render()");page.locator('#mediaInput').set_input_files({'name':'extra.png','mimeType':'image/png','buffer':(ROOT/'tests/fixtures/local-still.png').read_bytes()});page.wait_for_function('!ui.importing&&VaultBuddyQuality.getImportReport().length===1');r=ev('VaultBuddyQuality.getImportReport()[0]');expect(r['status']=='skipped' and '200' in r['message']);expect(ev('project.assets.length')==200);expect(not ev("[...media.values()].some(m=>m.file?.name==='extra.png')"))
 check('Source-cap errors report the limit without retaining a rejected media object',import_cap)
 def guide_dialog():
  ev("VaultBuddyGuide.start('audio')");page.wait_for_timeout(80);page.locator('[data-action=mixer]').first.click();page.wait_for_function('VaultBuddyGuide.getState().suspended');page.keyboard.press('Escape');page.wait_for_function('VaultBuddyGuide.getState().active&&!VaultBuddyGuide.getState().suspended',timeout=1200);expect(ev('VaultBuddyGuide.getState().step')=='audio')
 check('Dialog interruption resumes the current onboarding lesson after native dismissal',guide_dialog)
 def status_guide():
  ev("VaultBuddyGuide.start('save')");before=ev('JSON.stringify(project)');ev('Quality.showSession()');page.wait_for_function('VaultBuddyGuide.getState().suspended');expect(page.locator('#sessionDialog .guide-dialog-note').count()==1);page.keyboard.press('Escape');page.wait_for_function('!VaultBuddyGuide.getState().suspended');expect(ev('JSON.stringify(project)')==before)
 check('New session dialog participates in onboarding suspension without altering edits',status_guide)
 def peak():
  before=ev('JSON.stringify(project)');ev('Quality.observePeak(.999)');expect(page.locator('#peakMeter').evaluate('(e)=>e.classList.contains("peak-hot")'));expect('dBFS' in page.locator('#peakMeter').get_attribute('aria-label'));expect(ev('JSON.stringify(project)')==before)
 check('Observed audio-peak feedback is accessible and never modifies the mix',peak)
 def diag():
  with page.expect_download() as di:r=ev('diagnosticReport()')
  expect(r['product']=='vault-buddy-tutorial-editor' and r['build']=='implementation-reference');s=json.dumps(r);expect(ev('project.title') not in s);expect('privacy' in r);di.value.save_as(str(OUT/'diagnostics.json'))
 check('Diagnostics identify the product and omit project names, media paths and content',diag)
 def long_fit():
  ev("project.clips=[newClip('long','screen','v1','Long fixture',0,0,3000000)];project.effects=[];project.markers=[];project.transitions=[];project.assets.find(a=>a.id==='screen').duration_ms=3000000;ui.zoom=1;render()")
  geometry=ev("({lane:laneWidth(),visible:$('#timelineScroll').clientWidth-labelWidth(),labels:document.querySelectorAll('#ruler .ruler-tick').length,scale:pps()})");expect(geometry['lane']<=geometry['visible']+15,str(geometry));expect(geometry['labels']<30,str(geometry));return geometry
 check('Fit shows the full long edit instead of clamping to a 24,000-pixel timeline',long_fit)
 def film_cleanup():
  ev("Quality.syncFilms();window.__oldFilmProps=[...document.querySelector('#timelineContent').style].filter(k=>k.startsWith('--film-'));lastPackage={blob:new Blob(['old output'])};lastRenderContext={snapshot:project};Quality.releaseSessionMedia()");expect(ev("__oldFilmProps.length")>0);expect(ev("__oldFilmProps.every(k=>!document.querySelector('#timelineContent').style.getPropertyValue(k))"))
 check('Session cleanup also releases shared thumbnail CSS references',film_cleanup)
 def old_render_cleanup():
  ev("lastPackage={blob:new Blob(['old output'])};lastRenderContext={snapshot:project};pendingRange={start_ms:0,end_ms:1000};Quality.releaseSessionMedia()");expect(ev('lastPackage===null&&lastRenderContext===null&&pendingRange===null'))
 check('Replacing a session releases old rendered-package and pending-render references',old_render_cleanup)
 def relink_guard():
  ev("relinkBusy=true;newEmptyProject()");expect(page.locator('#confirmDialog').is_hidden());expect('reconnecting' in page.locator('#toast').inner_text());ev('resetDemo()');expect(page.locator('#confirmDialog').is_hidden());ev('relinkBusy=false')
 check('Project replacement cannot race an active source reconnection',relink_guard)
 def compact_guide_context():
  for w in [960,720]:
   page.set_viewport_size({'width':w,'height':640});ev("VaultBuddyGuide.start('context')");page.wait_for_function("!!document.querySelector('.timeline-toolbar [data-editor=more][data-guide-highlight]')");expect(page.locator('.timeline-toolbar [data-editor=more]').is_visible());page.locator('.timeline-toolbar [data-editor=more]').click();expect(page.locator('[role=menu]').is_visible());page.keyboard.press('Escape');expect(ev('VaultBuddyGuide.getState().active'));ev('VaultBuddyGuide.pause(false)')
  page.set_viewport_size({'width':1600,'height':1000})
 check('Compact guide highlights the visible More alternative and opens its real menu',compact_guide_context)
 def identity_filters():
  expect(ev("treatmentFilter({})")=='none');expect(ev("treatmentFilter({adjustments:FILTERS.none})")=='none')
  expect(ev("treatmentFilter({adjustments:FILTERS.mono})")=='contrast(1.12) grayscale(1)')
  result=ev("""()=>{const source=document.createElement('canvas');source.width=160;source.height=90;const a=source.getContext('2d');const grad=a.createLinearGradient(0,0,160,90);grad.addColorStop(0,'#ab7bc4');grad.addColorStop(1,'#248c78');a.fillStyle=grad;a.fillRect(0,0,160,90);const left=document.createElement('canvas'),right=document.createElement('canvas');left.width=right.width=160;left.height=right.height=90;const l=left.getContext('2d'),r=right.getContext('2d');l.filter='brightness(1) contrast(1) saturate(1) sepia(0) grayscale(0)';l.drawImage(source,0,0);r.filter=treatmentFilter({});r.drawImage(source,0,0);const x=l.getImageData(0,0,160,90).data,y=r.getImageData(0,0,160,90).data;return {maximum_delta:Math.max(...x.map((v,i)=>Math.abs(v-y[i])))};}""")
  expect(result['maximum_delta']<=1,result);return result
 check('Neutral color settings skip filter work without changing reference pixels',identity_filters)
 check('Reviewed quality flows have no uncaught JavaScript errors or runtime network requests',lambda:(expect(not errors,str(errors)),expect(not requests,str(requests))))
 b.close()
summary={s:sum(r['status']==s for r in results) for s in ['PASS','FAIL']};print('SUMMARY',summary,flush=True)
(ROOT/'tests/quality-results.json').write_text(json.dumps({'html_sha256':hashlib.sha256(HTML.encode()).hexdigest(),'summary':summary,'results':results,'errors':errors,'requests':requests},indent=2))
raise SystemExit(1 if summary['FAIL'] else 0)
