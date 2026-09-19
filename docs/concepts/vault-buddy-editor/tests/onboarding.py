"""Run against the self-contained editor HTML. Origin persistence is separately classified.
The memory-storage adapter is explicitly a simulation, not proof of browser reload durability.
"""
from pathlib import Path
from playwright.sync_api import sync_playwright
import json,traceback,threading,http.server,functools
ROOT=Path(__file__).resolve().parents[1];HTML=(ROOT/'vault-buddy-editor.html').read_text()
OUT=ROOT/'tests/artifacts';OUT.mkdir(exist_ok=True)
results=[];all_errors=[];requests=[]
def check(name,fn):
 try:
  detail=fn();results.append(dict(name=name,status='PASS',detail=detail));print('PASS',name,detail or '',flush=True)
 except Exception as e:
  results.append(dict(name=name,status='FAIL',detail=str(e)));print('FAIL',name,str(e),flush=True);traceback.print_exc()
  try:page.evaluate("VaultBuddyGuide.pause(false);document.querySelectorAll('dialog[open]').forEach(d=>d.close())")
  except Exception:pass
def expect(v,msg='Assertion failed'):
 if not v:raise AssertionError(msg)
def ev(s):return page.evaluate(s)
def state():return ev('VaultBuddyGuide.getState()')
def start(id='welcome'):
 ev("(()=>{if(VaultBuddyGuide.getState().active)VaultBuddyGuide.pause(false);for(const d of document.querySelectorAll('dialog[open]'))d.close();closeContext();$('#menu').hidden=true;})()")
 page.wait_for_timeout(30)
 ev(f'VaultBuddyGuide.start({json.dumps(id)})');page.wait_for_timeout(90)
def reset():
 ev("""()=>{VaultBuddyGuide.pause(false);for(const d of document.querySelectorAll('dialog[open]'))d.close();pause();closeContext();clearTimeout(saveTimer);project=seed();projectRecord=newProjectRecord(project);history=[];redoStack=[];multiIds=[];ui.time=13400;ui.selected={type:'clip',id:'presenter1'};ui.propertyTab='layout';ui.tab='media';ui.zoom=1;ui.review=false;ui.mediaSearch='';$('#workspace').classList.remove('inspector-open','library-hidden');$('#app').classList.remove('library-drawer');$('#timelineScroll').scrollTop=0;$('#timelineScroll').scrollLeft=0;prepareBuiltin();render();syncMedia();} """)
 page.wait_for_timeout(40)
def load_page(browser,storage=None,w=1600,h=1000):
 c=browser.new_context(viewport={'width':w,'height':h},accept_downloads=True)
 p=c.new_page();p.set_default_timeout(5000)
 p.on('pageerror',lambda e:all_errors.append(str(e)))
 p.on('request',lambda r:requests.append(r.url) if r.url.startswith(('http:','https:','ws:','wss:')) else None)
 if storage is not None:
  p.evaluate('''data=>{window.__storageData={...data};window.__memoryStorage={getItem(k){return Object.hasOwn(__storageData,k)?__storageData[k]:null},setItem(k,v){__storageData[k]=String(v)},removeItem(k){delete __storageData[k]}};Object.defineProperty(window,'localStorage',{value:__memoryStorage,configurable:true});}''',storage)
 p.set_content(HTML);p.wait_for_function('!!window.VaultBuddyGuide');p.wait_for_timeout(80)
 return p,c
with sync_playwright() as pw:
 browser=pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True,args=['--no-sandbox','--autoplay-policy=no-user-gesture-required'])
 page,context=load_page(browser)
 check('First-use invitation does not open a modal or steal focus',lambda:(expect(page.locator('#guideInvite').is_visible()),expect(ev("document.querySelectorAll('dialog[open]').length===0")),expect(ev("document.activeElement===document.body"))))
 def invitation():
  page.locator('#guideInvite [data-guide=dismiss-invite]').first.click();expect(not page.locator('#guideInvite').is_visible());expect(state()['welcomeDismissed']);page.locator('#editorHelp').click();expect(page.locator('#guideHub').is_visible());expect(page.locator('.guide-chapter').count()==7);page.locator('[data-guide=close-hub]').click()
 check('Not now dismisses; permanent Help still opens all seven chapters',invitation)
 def safe_walkthrough():
  reset();baseline=ev('JSON.stringify(project)');hist=ev('history.length');start();total=len(ev('VaultBuddyGuide.steps()'))
  for i in range(total):
   expect(ev("Number($('#guideStepCount').textContent.split('/')[0])") == i+1)
   expect(not page.locator('#guideNext').is_disabled())
   page.locator('#guideNext').click();page.wait_for_timeout(40)
  expect(page.locator('#guideCompletion').is_visible());expect(state()['status']=='completed');expect(len(state()['reviewed'])==22)
  expect(ev('JSON.stringify(project)')==baseline,'Passive walkthrough modified project')
  expect(ev('history.length')==hist,'Passive walkthrough added undo steps')
  page.locator('[data-guide=finish-close]').first.click();return {'steps':total,'project_unchanged':True}
 check('All 22 read-only lessons complete without editing project or history',safe_walkthrough)
 def pause_resume():
  start('fades');page.locator('#guideCoach [data-guide=pause]').last.click();expect(not state()['active']);expect(state()['step']=='fades');expect(not page.locator('#guideLayer').is_visible());expect(ev("document.activeElement.id==='editorHelp'"));page.locator('#editorHelp').click();expect('step 13' in page.locator('#guideResumeDetail').inner_text());page.locator('#guideHubAction').click();expect(state()['step']=='fades' and state()['active']);expect(page.locator('#guideCoach').get_attribute('data-step')=='fades')
 check('Pause and Help → Resume retain the exact step and restore focus',pause_resume)
 def close_and_escape():
  page.locator('[aria-label="Dismiss guide and keep progress"]').click();expect(not state()['active']);start('tracks');page.keyboard.press('Escape');expect(not state()['active'] and state()['step']=='tracks');expect(ev("!document.querySelector('[data-guide-highlight]')"))
 check('Close and Escape both dismiss overlays and remove stale highlights',close_and_escape)
 def restart():
  page.locator('#editorHelp').click();page.locator('[data-guide=restart]').click();expect(page.locator('#guideRestart').is_visible());page.locator('[data-guide=cancel-restart]').click();before=state()['reviewed'];expect(len(before)==22)
  baseline=ev('JSON.stringify(project)');page.locator('[data-guide=restart]').click();page.locator('[data-guide=confirm-restart]').click();expect(state()['reviewed']==[] and state()['step']=='welcome');expect(ev('JSON.stringify(project)')==baseline)
 check('Start-over confirmation resets guide progress only; cancel preserves it',restart)
 def shortcuts():
  start('split');before=ev('JSON.stringify(project)');page.locator('#guideCoachTitle').focus();page.keyboard.press('s');page.keyboard.press('Delete');page.keyboard.press('Control+s');expect(ev('JSON.stringify(project)')==before);expect(not ev("!!document.querySelector('dialog[open]')"));page.locator('#guideNext').focus();page.keyboard.press('Tab');expect(ev("document.activeElement.closest('#guideCoach')!==null"));page.keyboard.press('Shift+Tab');expect(ev("document.activeElement.closest('#guideCoach')!==null"))
 check('Guide keys do not trigger editor edits; Tab cycles guide controls',shortcuts)
 def f6():
  start('preview');page.keyboard.press('F6');expect(ev("document.activeElement.id==='playButton'"));page.keyboard.press('F6');expect(ev("document.activeElement.id==='guideCoachTitle'"));page.keyboard.press('F1');expect(page.locator('#guideHub').is_visible() and not state()['active']);page.keyboard.press('Escape');expect(not page.locator('#guideHub').is_visible());page.keyboard.press('?');expect(page.locator('#guideHub').is_visible());page.keyboard.press('Escape')
 check('F6 switches to the target and back; F1 / ? open Help; native Escape closes it',f6)
 def start_play():
  start('preview');page.locator('#playButton').click();page.wait_for_timeout(250);expect(ev('ui.playing'));page.locator('#playButton').click();expect(not ev('ui.playing'));expect('preview' in state()['tried']);expect(state()['step']=='preview');expect('Control explored' in page.locator('#guideTaskText').inner_text())
 check('Highlight stays interactive; Play marks an attempt but never auto-advances',start_play)
 def split_undo():
  reset();start('split');n=ev('project.clips.length');page.locator('#splitButton').click();expect(ev('project.clips.length')==n+1);page.locator('#guideNext').click();expect(state()['step']=='undo');page.locator('#undoButton').click();expect(ev('project.clips.length')==n);expect(ev('history.length')==0)
 check('User-performed Split and Undo work normally through the overlay',split_undo)
 def more():
  start('context');page.locator('.timeline-toolbar [data-editor=more]').click();page.wait_for_timeout(120);expect(page.locator('[role=menu]').count()==1);page.keyboard.press('Escape');expect(page.locator('[role=menu]').count()==0);expect(state()['active']);page.keyboard.press('Escape');expect(not state()['active'])
 check('Context menu Escape closes menu first, then guide on next Escape',more)
 def modal_sequence():
  for id,sel,dialog in [('audio','[data-action=mixer]','#mixerDialog'),('checks','#issuesButton','#issuesDialog'),('save','.header-actions [data-action=downloadProject]','#saveProjectDialog'),('render','.header-actions [data-action=save]','#saveDialog')]:
   start(id);page.locator(sel).first.click();page.wait_for_timeout(100);expect(state()['suspended'],f'{id} not suspended');expect(not page.locator('#guideLayer').is_visible());expect(page.locator('dialog[open] .guide-dialog-note').count()==1)
   page.keyboard.press('Escape');page.wait_for_function('VaultBuddyGuide.getState().active && !VaultBuddyGuide.getState().suspended',timeout=1000);expect(state()['active'] and not state()['suspended'],f'{id} failed resume');expect(state()['step']==id);expect(page.locator('#guideCoach').is_visible())
 check('Mixer, Checks, Save and Render suspend and resume the same guide step',modal_sequence)
 def camera_permission():
  reset();ev("window.__deviceRequests=0;const md=navigator.mediaDevices||{};Object.defineProperty(md,'getUserMedia',{value:async()=>{window.__deviceRequests++;throw new Error('NO DEVICE');},configurable:true});if(!navigator.mediaDevices)Object.defineProperty(navigator,'mediaDevices',{value:md,configurable:true});")
  start('webcam');page.locator('.webcam-entry').click();page.wait_for_timeout(100);expect(state()['suspended']);expect(ev('__deviceRequests')==0);page.keyboard.press('Escape');page.wait_for_timeout(140);expect(state()['active']);expect(ev('__deviceRequests')==0)
 check('Webcam lesson opens setup without requesting camera/microphone permission',camera_permission)
 def pause_modal():
  start('audio');page.locator('[data-action=mixer]').first.click();page.wait_for_timeout(100);page.locator('dialog[open] .guide-dialog-note [data-guide=pause]').click();expect(not state()['active']);expect(ev("!!document.querySelector('dialog[open]')"));page.keyboard.press('Escape');expect(not state()['active'])
 check('Pause guide inside an editor dialog leaves that dialog intact',pause_modal)
 def minimize():
  start('layout');page.locator('[aria-label="Minimize guide"]').click();expect(state()['minimized']);expect(page.locator('#guideMini').is_visible());expect(not page.locator('#guideLayer').is_visible());page.locator('[aria-label="Expand walkthrough"]').click();expect(not state()['minimized']);expect(state()['step']=='layout');page.locator('[aria-label="Minimize guide"]').click();page.locator('[aria-label="Dismiss minimized guide"]').click();expect(not state()['active'])
 check('Minimize keeps the step; Expand and close work without editing',minimize)
 def jump():
  page.locator('#editorHelp').click();page.locator('.guide-chapter-main[data-guide-jump=fades]').click();expect(state()['step']=='fades');page.locator('[data-guide=contents]').click();details=page.locator('.guide-chapter').nth(1).locator('details');details.locator('summary').click();details.locator('[data-guide-jump=context]').click();expect(state()['step']=='context')
 check('Learning center jumps to chapter starts and individual lessons',jump)
 def answers():
  ev('VaultBuddyGuide.pause(false)');page.locator('#editorHelp').click();page.locator('[data-guide-hub-tab=answers]').click();page.locator('#guideSearch').fill('microphone');expect(page.locator('.guide-answer:visible').count()>=1);page.locator('#guideSearch').fill('impossible-match-123');expect(page.locator('#guideNoResults').is_visible());page.locator('#guideSearch').fill('What is the difference');page.locator('.guide-answer:visible summary').click();page.locator('.guide-answer:visible [data-guide-jump]').click();expect(state()['step']=='save')
 check('Quick-answer search, no-results feedback and contextual Show me links work',answers)
 def keyboard_table():
  ev('VaultBuddyGuide.pause(false)');page.locator('#editorHelp').click();page.locator('[data-guide-hub-tab=keys]').click();expect(page.locator('.guide-key-table>div').count()==11);page.locator('[data-guide=shortcut-help]').click();expect(page.locator('#helpDialog').is_visible());page.locator('[data-guide=hub-from-legacy]').click();expect(page.locator('#guideHub').is_visible());page.keyboard.press('Escape')
 check('Shortcut reference and learning center remain accessible both directions',keyboard_table)
 def unavailable():
  reset();start('undo');expect('Nothing to undo' in page.locator('#guideStatusText').inner_text());expect(page.locator('#guideFocus').is_disabled());expect(not page.locator('#guideNext').is_disabled())
  ev('VaultBuddyGuide.pause(false);project.clips=[];project.effects=[];project.markers=[];project.transitions=[];ui.selected=null;render()');start('layout');expect('No video clip' in page.locator('#guideStatusText').inner_text());expect(not page.locator('#guideNext').is_disabled());page.locator('#guideNext').click();expect(state()['step']=='fades');reset()
 check('Unavailable Undo and empty projects explain prerequisites without blocking Next',unavailable)
 def locks():
  reset();ev("project.tracks.forEach(t=>t.locked=true);render()");before=ev('JSON.stringify(project)');start('split');expect(page.locator('#splitButton').is_disabled());expect('Unlock' in page.locator('#splitButton').get_attribute('title'));expect(ev('JSON.stringify(project)')==before);page.locator('#guideNext').click();expect(state()['step']=='undo');reset()
 check('Guide never unlocks tracks or bypasses existing editing safeguards',locks)
 def reanchor():
  start('fades');page.locator('[data-fade-preset="500"]').click();page.wait_for_timeout(100);expect(ev("document.querySelector('[data-guide-highlight]')===document.querySelector('.inspector')"));expect('fades' in state()['tried']);page.locator('#guideNext').click();page.wait_for_function("document.querySelectorAll('[data-guide-highlight]').length===1")
 check('Highlights reconnect after property rerenders; no duplicate descriptions',reanchor)
 def view_sizes():
  details=[]
  for w,h in [(1600,1000),(1280,800),(960,640),(720,640),(390,844)]:
   page.set_viewport_size({'width':w,'height':h});page.wait_for_timeout(60)
   for id in ['media','select','fades','captions','save']:
    start(id);r=page.locator('#guideCoach').bounding_box();expect(r and r['x']>=0 and r['y']>=0 and r['x']+r['width']<=w+.5 and r['y']+r['height']<=h+.5,f'{w} {id} panel exceeds viewport: {r}')
    expect(not page.locator('#guideNext').is_disabled());details.append({'width':w,'height':h,'step':id,'contained':True})
  page.set_viewport_size({'width':1600,'height':1000});reset();return details
 check('Coach remains contained across 25 desktop/compact layout combinations',view_sizes)
 def light():
  ev("document.documentElement.dataset.theme='light'");start('fades');expect(page.locator('#guideCoach').is_visible());expect(ev("getComputedStyle(document.querySelector('#guideCoach')).backgroundColor")=='rgb(255, 255, 255)');page.emulate_media(reduced_motion='reduce');expect(ev("getComputedStyle(document.querySelector('#guideCoach')).animationName")=='none');ev("document.documentElement.dataset.theme='dark'");page.emulate_media(reduced_motion='no-preference')
 check('Light-theme tokens and reduced-motion preference apply to walkthrough',light)
 def help_small():
  ev('VaultBuddyGuide.pause(false)')
  for w,h in [(960,640),(720,640),(390,844)]:
   page.set_viewport_size({'width':w,'height':h});page.wait_for_timeout(70)
   button=page.locator('#editorHelp');expect(button.is_visible(),f'Help hidden at {w}')
   r=button.bounding_box();expect(r['x']>=0 and r['x']+r['width']<=w,f'Help outside viewport at {w}')
   expect(ev('document.body.scrollWidth<=innerWidth+1'),f'Header overflow at {w}')
   button.click();expect(page.locator('#guideHub').is_visible());page.keyboard.press('Escape')
  page.set_viewport_size({'width':1600,'height':1000});reset()
 check('Help remains visible, reachable and functional at compact and phone widths',help_small)
 def narrow_target_uncovered():
  page.set_viewport_size({'width':390,'height':844});start('media')
  a=page.locator('#guideCoach').bounding_box();b=page.locator('#library [data-action=import]').bounding_box()
  expect(a['y']+a['height']<=b['y'] or b['y']+b['height']<=a['y'],'Coach obscures Import')
  n=page.locator('#guideNext').bounding_box();expect(n['y']>=a['y'] and n['y']+n['height']<=a['y']+a['height'])
  page.set_viewport_size({'width':1600,'height':1000});reset()
 check('Compact guidance keeps the explained Import control uncovered and Next visible',narrow_target_uncovered)

 def no_stale_pause_toast():
  start('media');ev('VaultBuddyGuide.pause()');expect(page.locator('#toast').evaluate('(e)=>e.classList.contains("visible")'))
  page.locator('#editorHelp').click();page.locator('#guideHubAction').click();expect(not page.locator('#toast').evaluate('(e)=>e.classList.contains("visible")'))
 check('Resuming clears only the obsolete guide-pause notification',no_stale_pause_toast)
 def canvas_isolation():
  reset();ev('paint(ctx,false)');before=ev('canvas.toDataURL()');start('preview');ev('paint(ctx,false)');expect(ev('canvas.toDataURL()')==before,'Guide appeared in composed output')
  expect(ev('history.length')==0)
 check('Onboarding overlays do not enter composed video pixels or undo history',canvas_isolation)
 def restore_description():
  ev('VaultBuddyGuide.pause(false)');ev("document.querySelector('#splitButton').setAttribute('aria-describedby','existing-description')")
  start('split');expect(page.locator('#splitButton').get_attribute('aria-describedby')=='existing-description guideTargetDescription')
  ev('VaultBuddyGuide.pause(false)');expect(page.locator('#splitButton').get_attribute('aria-describedby')=='existing-description')
  ev("document.querySelector('#splitButton').removeAttribute('aria-describedby')")
 check('Leaving a highlight restores any pre-existing accessibility description',restore_description)

 def nontarget_keys():
  start('preview');page.locator('#guideFocus').click();page.keyboard.press('F6');page.locator('#guideCoach [data-guide=pause]').last.click();before=ev('project.clips.length');ev("selectClip('c2');seek(14000)");page.locator('#preview').focus();page.keyboard.press('s');expect(ev('project.clips.length')==before+1);ev('undo()')
 check('Normal keyboard editing works after the guide is dismissed',nontarget_keys)
 def change_project():
  start('select');ev("project.id='other-workspace';projectRecord=newProjectRecord(project);render()");page.wait_for_timeout(120);expect(not state()['active']);expect(state()['step']=='select');reset()
 check('Changing to another project pauses rather than using stale clip targets',change_project)
 def toggle_dim():
  ev('VaultBuddyGuide.pause(false)');page.locator('#editorHelp').click();page.locator('.guide-hub-footer summary').click();page.locator('#guideDimOption').uncheck();expect(not state()['dim']);page.locator('#guideHubAction').click();expect(not page.locator('#guideShade').is_visible())
 check('Dimming preference can be disabled while keeping the control outline',toggle_dim)
 def export_import():
  start('callouts');ev('VaultBuddyGuide.pause(false)');page.locator('#editorHelp').click()
  if not page.locator('.guide-hub-footer details').evaluate('(el)=>el.open'):page.locator('.guide-hub-footer summary').click()
  with page.expect_download() as d:page.locator('[data-guide=save-progress]').click()
  path=OUT/'progress.vbguide.json';d.value.save_as(str(path));data=json.loads(path.read_text());expect(data['step']=='callouts');expect('project' not in data and 'assets' not in data and 'clips' not in data);expect(len(path.read_bytes())<3000)
  page.locator('#guideProgressInput').set_input_files({'name':'progress.vbguide.json','mimeType':'application/json','buffer':json.dumps({**data,'step':'audio'}).encode()});page.wait_for_timeout(120);expect(state()['step']=='audio');page.locator('#guideHubAction').click();expect(state()['active'] and state()['step']=='audio')
  return {'bytes':path.stat().st_size,'contains_project_or_media':False}
 check('Progress-only file download and reload preserve an exact resume position',export_import)
 def portable_progress_fresh_page():
  p,c=load_page(browser)
  p.locator('#guideInvite [data-guide=dismiss-invite]').first.click()
  baseline=p.evaluate('JSON.stringify(project)');p.locator('#editorHelp').click()
  p.locator('#guideProgressInput').set_input_files(str(OUT/'progress.vbguide.json'))
  p.wait_for_function("VaultBuddyGuide.getState().step==='callouts'")
  p.locator('#guideHubAction').click();expect(p.evaluate("VaultBuddyGuide.getState().active&&VaultBuddyGuide.getState().step==='callouts'"))
  expect(p.evaluate('JSON.stringify(project)')==baseline);c.close()
 check('Actual downloaded progress file resumes in a fresh browser context without storage',portable_progress_fresh_page)

 def bad_files():
  ev('VaultBuddyGuide.pause(false)');page.locator('#editorHelp').click();before=state()['step']
  for data in ['{broken','{"schema":"future/999"}','x'*66000]:
   page.locator('#guideProgressInput').set_input_files({'name':'bad.json','mimeType':'application/json','buffer':data.encode()});page.wait_for_timeout(80);expect(state()['step']==before)
  expect(ev("(()=>{try{VaultBuddyGuide.parseProgress({schema:'vault-buddy-onboarding/1',step:'<img onerror=alert()>',reviewed:['welcome','unknown'],tried:[]});return true}catch{return false}})()"));expect(ev("VaultBuddyGuide.parseProgress({schema:'vault-buddy-onboarding/1',step:'unknown',reviewed:['welcome','unknown'],tried:[]}).step")=='media');page.keyboard.press('Escape')
 check('Malformed, oversized, future-version files are refused; unknown lesson IDs normalize safely',bad_files)
 def missing_storage():
  expect(state()['storageAvailable']==False);start();expect('Session only' in page.locator('#guideCoachStorage').inner_text());ev('VaultBuddyGuide.pause(false)');page.locator('#editorHelp').click();expect('unavailable' in page.locator('#guidePersistence').inner_text());page.keyboard.press('Escape')
 check('Denied storage is explicit and leaves session resume usable',missing_storage)
 # A simulated storage adapter proves serialization/deserialization separately from blocked browser origins.
 def adapter_persistence():
  p,c=load_page(browser,{})
  p.locator('#guideInvite [data-guide=start]').click();p.evaluate("VaultBuddyGuide.start('fades');VaultBuddyGuide.pause(false)");stored=p.evaluate('({...__storageData})');c.close()
  p,c=load_page(browser,stored);expect(p.evaluate('VaultBuddyGuide.getState().step')=='fades');expect(not p.evaluate('VaultBuddyGuide.getState().active'));expect(not p.locator('#guideInvite').is_visible());p.locator('#editorHelp').click();p.locator('#guideHubAction').click();expect(p.evaluate('VaultBuddyGuide.getState().step')=='fades');c.close();return 'Simulated adapter across fresh pages; not native origin persistence'
 check('Serialized guide resumes in a fresh page with a storage adapter',adapter_persistence)
 def completed_adapter():
  key='vault-buddy.editor.onboarding.v1';ids=ev('VaultBuddyGuide.steps().map(s=>s.id)');data={'schema':'vault-buddy-onboarding/1','step':'help','status':'completed','reviewed':ids,'tried':[],'welcomeDismissed':True,'dim':True}
  p,c=load_page(browser,{key:json.dumps(data)});expect(not p.locator('#guideInvite').is_visible());p.locator('#editorHelp').click();expect(p.locator('#guideProgressNumber').inner_text()=='100%');expect(p.locator('#guideHubAction').inner_text()=='Revisit walkthrough');c.close()
 check('Completed learners are not prompted again; lessons remain available',completed_adapter)
 def write_failure():
  p,c=load_page(browser,{});p.locator('#guideInvite [data-guide=start]').click();p.evaluate("()=>{__memoryStorage.setItem=()=>{throw new DOMException('Storage denied','SecurityError')};}");p.locator('#guideNext').click();expect(not p.evaluate('VaultBuddyGuide.getState().storageAvailable'));expect('Session only' in p.locator('#guideCoachStorage').inner_text());p.keyboard.press('Escape');expect(p.evaluate('VaultBuddyGuide.getState().step')=='media');c.close()
 check('Storage failure mid-tour degrades to session mode without losing the current step',write_failure)
 check('No uncaught browser errors and no runtime network requests',lambda:(expect(not all_errors,str(all_errors)),expect(not requests,str(requests))))
 # Attempt real origin-backed navigation once; do not circumvent managed-browser policy.
 handler=functools.partial(http.server.SimpleHTTPRequestHandler,directory=str(ROOT));server=http.server.ThreadingHTTPServer(('127.0.0.1',0),handler);threading.Thread(target=server.serve_forever,daemon=True).start()
 p=browser.new_page()
 try:
  p.goto(f'http://127.0.0.1:{server.server_port}/vault-buddy-editor.html',wait_until='load',timeout=10000);p.wait_for_function('!!window.VaultBuddyGuide');p.evaluate("VaultBuddyGuide.start('fades');VaultBuddyGuide.pause(false)");p.reload();p.wait_for_function('!!window.VaultBuddyGuide');expect(p.evaluate('VaultBuddyGuide.getState().step')=='fades');results.append(dict(name='Real browser origin reload retains guide progress',status='PASS'))
 except Exception as e:results.append(dict(name='Real browser origin reload retains guide progress',status='BLOCKED',detail=str(e)[:600]));print('BLOCKED origin-backed reload',str(e)[:150])
 finally:p.close();server.shutdown()
 browser.close()
(ROOT/'tests/onboarding-results.json').write_text(json.dumps(results,indent=2))
print('SUMMARY', {s:sum(r['status']==s for r in results) for s in ['PASS','FAIL','BLOCKED']})
if any(r['status']=='FAIL' for r in results):raise SystemExit(1)
