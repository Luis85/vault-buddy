"""Workspace UI behavior, command reachability and geometry. No patched application code."""
from pathlib import Path
from playwright.sync_api import sync_playwright
import json,traceback,time
ROOT=Path(__file__).resolve().parents[1]
results=[];errors=[];requests=[]
def expect(v,msg='Assertion failed'):
 if not v:raise AssertionError(msg)
def ev(js):return page.evaluate(js)
def reset():
 page.set_viewport_size({'width':1600,'height':1000})
 ev('''()=>{VaultBuddyGuide.pause(false);for(const d of document.querySelectorAll('dialog[open]'))d.close();pause();closeContext();project=seed();projectRecord=newProjectRecord(project);history=[];redoStack=[];multiIds=[];ui.time=13400;ui.tab='media';ui.propertyTab='layout';ui.selected={type:'clip',id:'presenter1'};ui.review=false;ui.exporting=false;projectBusy=false;ui.zoom=1;VaultBuddyFocus.resetView();document.documentElement.dataset.theme='dark';setTimelineHeight(400);prepareBuiltin();render();syncMedia();}''')
 page.wait_for_timeout(80)
def check(name,fn):
 try:
  reset();detail=fn();results.append({'name':name,'status':'PASS','detail':detail});print('PASS',name,detail or '',flush=True)
 except Exception as e:
  results.append({'name':name,'status':'FAIL','detail':str(e)});print('FAIL',name,str(e),flush=True)
  traceback.print_exc();page.screenshot(path=str(ROOT/f'tests/artifacts/focus-fail-{len(results)}.png'))
def menu(button,name,role='menuitem'):
 page.locator(button).click();page.get_by_role(role,name=name,exact=True).click()
with sync_playwright() as p:
 b=p.chromium.launch(executable_path='/usr/bin/chromium',headless=True,args=['--no-sandbox','--autoplay-policy=no-user-gesture-required'])
 ctx=b.new_context(viewport={'width':1600,'height':1000},accept_downloads=True);page=ctx.new_page();page.set_default_timeout(4000)
 page.on('pageerror',lambda e:errors.append(str(e)));page.on('request',lambda r:requests.append(r.url) if r.url.startswith(('http:','https:','ws:','wss:')) else None)
 page.set_content((ROOT/'vault-buddy-editor.html').read_text());page.wait_for_function('!!window.VaultBuddyFocus');page.locator('#guideInvite [data-guide=dismiss-invite]').first.click()
 def one_header():
  expect(page.locator('.center > .toolstrip').count()==0);expect(page.locator('.preview-header .toolstrip').count()==1)
  expect(not page.locator('#selectionBar').is_visible());expect(page.locator('#selectionBar button').count()==0)
  rects=ev('''()=>Object.fromEntries(['.preview-header','.toolstrip','#canvasWrap'].map(s=>{const r=document.querySelector(s).getBoundingClientRect();return[s,{x:r.x,y:r.y,w:r.width,h:r.height}]}))''')
  expect(rects['.preview-header']['h']==48,rects);expect(abs(rects['#canvasWrap']['w']/rects['#canvasWrap']['h']-16/9)<.001,rects)
  return rects
 check('Teaching toolbar is inside one 48px preview header; preview keeps its aspect ratio',one_header)
 def duplicate_count():
  data=ev('''()=>Object.fromEntries(['[data-action="downloadProject"]','[data-action="webcam"]','[data-effect="text"]','[data-effect="arrow"]','[data-editor="more"]'].map(s=>[s,[...document.querySelectorAll(s)].filter(e=>e.getClientRects().length&&!e.closest('dialog:not([open])')).length]))''')
  expect(all(v==1 for v in data.values()),data);return data
 check('Save, webcam, text, arrow and selection actions each have one persistent entry',duplicate_count)
 def nav():
  r=page.locator('.tabs button').evaluate_all('(es)=>es.map(e=>({text:e.innerText,y:e.getBoundingClientRect().y}))')
  expect([v['text'] for v in r]==['Media','Titles','Captions','Chapters']);expect(len(set(v['y'] for v in r))==1,r);return r
 check('Four distinct library destinations occupy one row',nav)
 def project_rename():
  page.locator('.document-name').click();expect(page.locator('#editDialog').is_visible());page.fill('#editText','Fast tutorial');page.locator('#editForm [type=submit]').click();expect(ev("project.title==='Fast tutorial'"));expect(page.locator('#projectTitle').inner_text()=='Fast tutorial');ev('undo()');expect(ev("project.title==='Create your first project'"))
 check('Inline project title still renames and participates in Undo',project_rename)
 def project_keys():
  page.locator('#projectMenuButton').focus();page.keyboard.press('ArrowDown');expect(page.locator('#focusCommandMenu').is_visible());expect(ev("document.activeElement.textContent.includes('New project')"));page.keyboard.press('End');expect(ev("document.activeElement.textContent.includes('Restore sample')"));page.keyboard.press('Escape');expect(ev("document.activeElement.id==='projectMenuButton'"));expect(page.locator('#projectMenuButton').get_attribute('aria-expanded')=='false')
 check('Project menu supports arrow entry, End, Escape and focus restoration',project_keys)
 def repeat_click():
  page.locator('#moreToolsButton').click();expect(page.locator('#focusCommandMenu').is_visible());page.locator('#moreToolsButton').click();expect(page.locator('[role=menu]').count()==0,'Second click should close the same menu')
 check('Clicking the same menu trigger twice closes it',repeat_click)
 def read_only():
  original=ev('JSON.stringify(project)');token=ev('documentToken()')
  menu('#projectMenuButton','Workspace & rendered products');expect(ev("ui.tab==='project'"));page.locator('.library-back').click()
  menu('#viewMenuButton','Light theme','menuitemcheckbox');menu('#viewMenuButton','Show properties','menuitemcheckbox');menu('#viewMenuButton','Reset panel layout')
  expect(ev('JSON.stringify(project)')==original);expect(ev('documentToken()')==token);expect(ev('history.length')==0)
 check('Project and view navigation never create a project edit or revision',read_only)
 def rare_tool():
  ev("selectClip('c2');seek(13000)");n=ev('project.effects.length');menu('#moreToolsButton','Spotlight');expect(ev("project.effects.at(-1).kind==='spotlight'"));ev('undo()');expect(ev('project.effects.length')==n);menu('#moreToolsButton','Numbered step');expect(ev("project.effects.at(-1).kind==='step'"));ev('undo()');menu('#moreToolsButton','Privacy cover');expect(ev("project.effects.at(-1).kind==='mask'"));expect('source' in page.locator('#inspector').inner_text().lower())
 check('Spotlight, numbered step and privacy cover dispatch real undoable commands',rare_tool)
 def overflow():
  out=[]
  for w,h in [(1600,1000),(1280,800),(960,640),(720,640),(390,844)]:
   page.set_viewport_size({'width':w,'height':h});page.wait_for_timeout(100)
   data=ev('''()=>{const b=document.querySelector('.preview-header').getBoundingClientRect();return{width:innerWidth,body:document.body.scrollWidth,bar:{x:b.x,y:b.y,w:b.width,h:b.height},controls:[...document.querySelectorAll('.preview-header button')].filter(e=>e.getClientRects().length).map(e=>{const r=e.getBoundingClientRect();return{name:e.title||e.textContent,x:r.x,y:r.y,w:r.width,h:r.height}})}}''')
   expect(data['body']<=w+1,data)
   for c in data['controls']:
    expect(c['y']>=data['bar']['y']-.5 and c['y']+c['h']<=data['bar']['y']+data['bar']['h']+.5,data)
    expect(c['x']>=data['bar']['x']-.5 and c['x']+c['w']<=data['bar']['x']+data['bar']['w']+.5,data)
   page.locator('#moreToolsButton').click();labels=page.locator('#focusCommandMenu [role=menuitem]').all_text_contents()
   for key,label in [('text','Text'),('arrow','Arrow'),('highlight','Highlight'),('zoom','Zoom')]:
    if not page.locator('.toolstrip [data-effect='+key+']').is_visible():expect(any(t.strip()==label for t in labels),labels)
   page.keyboard.press('Escape');out.append({'viewport':w,'height':data['bar']['h'],'commands_fit':True,'overflow_reachable':True})
  return out
 check('At five viewport sizes the header remains one row and overflowed tools stay reachable',overflow)
 def narrow_tool():
  page.set_viewport_size({'width':390,'height':844});page.wait_for_timeout(100);ev("selectClip('c2');seek(13000)");menu('#moreToolsButton','Zoom');expect(ev("project.effects.at(-1).kind==='zoom'"))
 check('Zoom works through the overflow menu in a narrow viewport',narrow_tool)
 def camera():
  page.locator('.webcam-entry').click();expect(page.locator('#webcamDialog').is_visible());expect(ev("webcamState.phase==='idle'&&!webcamState.stream"));page.locator('#webcamDialog [data-cam=close]').first.click();expect(not page.locator('#webcamDialog').is_visible())
 check('Webcam has one Media entry and opening it does not start capture',camera)
 def import_file():
  with page.expect_file_chooser() as choice:page.locator('.media-actions .import').click()
  choice.value.set_files(str(ROOT/'tests/fixtures/narration-tone.wav'));page.wait_for_function("project.assets.some(a=>a.name==='narration-tone.wav')");expect(page.locator('.import-summary').is_visible());expect(page.locator('.media-actions .import').is_visible())
 check('Relocated Import opens the real picker and imports a real audio file',import_file)
 def precision():
  ds=page.locator('.precision-section').first
  if ds.get_attribute('open') is not None:ds.locator('summary').click()
  expect(not page.locator('[data-video-shape=circle]').is_visible());ds.locator('summary').click();page.locator('[data-video-shape=circle]').click();expect(ev("selectedClip().frame_shape==='circle'"));expect(page.locator('[data-video-shape=circle]').is_visible());page.locator('[data-prop=videoMirror]').check();expect(ev('selectedClip().mirror'));expect(page.locator('[data-prop=videoMirror]').is_visible())
 check('Advanced framing is disclosed explicitly and stays open while editing',precision)
 def edit_menu():
  page.locator('.timeline-toolbar [data-editor=more]').click();labels=page.locator('[role=menu]').inner_text();page.keyboard.press('Escape');page.locator('.clip[data-clip=presenter1]').click(button='right');labels2=page.locator('[role=menu]').inner_text();expect('Rename' in labels and 'Rename' in labels2);page.keyboard.press('Escape')
 check('Edit actions and right-click retain the same object-specific command family',edit_menu)
 def categories():
  for tab in ['fades','audio','speed','color','properties','layout']:
   page.locator(f'[data-property-tab={tab}]').click();expect(ev('ui.propertyTab')==tab)
  page.locator('[data-property-tab=fades]').click();page.locator('[data-fade-preset="500"]').click();expect(ev('selectedClip().fade_in_ms')==500)
 check('All six property categories remain available; fades still change the real clip',categories)
 def roving():
  a=ev('ui.time');page.locator('.toolstrip [data-effect=text]').focus();page.keyboard.press('ArrowRight');expect(ev("document.activeElement.dataset.effect==='arrow'"));page.keyboard.press('End');expect(ev("document.activeElement.id==='moreToolsButton'"));page.keyboard.press('Home');expect(ev("document.activeElement.dataset.effect==='text'"));expect(ev('ui.time')==a);expect(page.locator('.toolstrip [tabindex="0"]').count()==1)
 check('Preview tools have a single tab stop and arrow navigation without moving the playhead',roving)
 def panels():
  page.locator('#inspectorToggle').click();expect(not page.locator('.inspector').is_visible());page.locator('#inspectorToggle').click();expect(page.locator('.inspector').is_visible());page.locator('#libraryToggle').click();expect(not page.locator('.sidebar').is_visible());page.locator('#libraryToggle').click();expect(page.locator('.sidebar').is_visible());page.set_viewport_size({'width':960,'height':640});page.wait_for_timeout(100);expect(not page.locator('.inspector').is_visible());page.locator('#inspectorToggle').click();expect(page.locator('.inspector').is_visible());page.locator('#inspectorToggle').click();expect(not page.locator('.inspector').is_visible())
 check('Library and property toggles work in docked and compact layouts',panels)
 def focus_view():
  before=ev('JSON.stringify(project)');menu('#viewMenuButton','Focus preview','menuitemcheckbox');expect(not page.locator('.sidebar').is_visible());expect(not page.locator('.inspector').is_visible());expect(page.locator('#splitButton').is_visible());menu('#viewMenuButton','Reset panel layout');expect(page.locator('.sidebar').is_visible());expect(page.locator('.inspector').is_visible());expect(ev('JSON.stringify(project)')==before)
 check('Focus preview and Reset layout change workspace visibility only',focus_view)
 def save_render():
  page.locator('.header-actions [data-action=downloadProject]').click();expect(page.locator('#projectSaveDialog').is_visible());page.locator('#projectSaveDialog [data-action=close]').first.click();page.locator('.header-actions [data-action=save]').click();expect(page.locator('#saveDialog').is_visible());expect(not ev('ui.exporting'));page.locator('#saveDialog [data-action=close]').first.click()
 check('Saving the editable project stays distinct from rendering the video product',save_render)
 def guide_targets():
  for step,selector in [('welcome','.projectbar'),('context','.timeline-toolbar [data-editor=more]'),('webcam','.webcam-entry'),('callouts','.toolstrip')]:
   ev(f"VaultBuddyGuide.start('{step}')");page.wait_for_timeout(220);expect(page.locator(selector).is_visible());expect(page.locator(selector).get_attribute('data-guide-highlight')=='true',step);ev('VaultBuddyGuide.pause(false)')
 check('Relocated guide lessons highlight the actual live controls',guide_targets)
 def guide_webcam():
  ev("VaultBuddyGuide.start('webcam')");page.wait_for_function("!!document.querySelector('.webcam-entry')");page.locator('.webcam-entry').click();page.wait_for_function('VaultBuddyGuide.getState().suspended');page.keyboard.press('Escape');page.wait_for_function("VaultBuddyGuide.getState().active && !VaultBuddyGuide.getState().suspended");expect(ev("VaultBuddyGuide.getState().step==='webcam'"))
 check('Onboarding suspends for webcam setup and resumes the same lesson',guide_webcam)
 def guide_tools():
  before=ev('JSON.stringify(project)');ev("VaultBuddyGuide.start('callouts')");page.wait_for_timeout(100);page.locator('#moreToolsButton').click();page.wait_for_timeout(100);expect(ev('VaultBuddyGuide.getState().suspended'));page.keyboard.press('Escape');page.wait_for_timeout(120);expect(ev('VaultBuddyGuide.getState().active'));expect(ev('JSON.stringify(project)')==before)
 check('More tools remains compatible with tutorial suspension and resume',guide_tools)
 def workspace():
  page.locator('#inspectorToggle').click();ev("$('#app').classList.add('focus-view')");w=ev('workspaceEnvelope()');expect(w['workspace']['properties_hidden']);expect(w['workspace']['focus_preview']);page.evaluate('w=>{VaultBuddyFocus.resetView();const p=validateWorkspace(w);applyWorkspace(p.workspace)}',w);expect(ev("$('#workspace').classList.contains('properties-hidden') && $('#app').classList.contains('focus-view')"))
 check('New view preferences validate and round-trip in the editable workspace envelope',workspace)
 def aspect():
  out=[]
  for f in ['landscape','portrait','square','classic']:
   # Use actual format names exposed by the implementation rather than assume aliases.
   pass
  for key in ev('Object.keys(FORMATS)'):
   ev(f"canvasFormat('{key}')");page.wait_for_timeout(80);data=ev('''()=>{const r=document.querySelector('#canvasWrap').getBoundingClientRect();return{w:r.width,h:r.height,expected:W/H}}''');expect(abs(data['w']/data['h']-data['expected'])<.004,data);out.append(data)
  return out
 check('All four output formats preserve correct display proportions after the toolbar move',aspect)
 def review():
  page.locator('#reviewButton').click();expect(ev('ui.review'));expect(page.locator('#reviewButton').is_visible());expect('Back to edit' in page.locator('#reviewButton').inner_text());expect(not page.locator('.toolstrip').is_visible());page.locator('#reviewButton').click();expect(not ev('ui.review'));expect(page.locator('.toolstrip').is_visible())
 check('Review retains a visible Back to edit action in the same header',review)
 def new_project():
  before=ev('JSON.stringify(project)');menu('#projectMenuButton','New project…');expect(page.locator('#confirmDialog').is_visible());page.locator('#confirmDialog [data-action=close]').click();expect(ev('JSON.stringify(project)')==before);menu('#projectMenuButton','New project…');page.locator('#confirmButton').click();expect(ev('project.clips.length')==0);expect(page.locator('.media-actions .import').is_visible());expect(page.locator('.webcam-entry').is_visible())
 check('New project keeps its confirmation and leads directly to import or webcam',new_project)
 def menu_fit():
  page.set_viewport_size({'width':390,'height':600});page.wait_for_timeout(100)
  for id in ['projectMenuButton','viewMenuButton','moreToolsButton']:
   page.locator('#'+id).click();r=page.locator('#focusCommandMenu').bounding_box();expect(r['x']>=0 and r['x']+r['width']<=390.5 and r['y']>=0 and r['y']+r['height']<=600.5,r);page.keyboard.press('Escape')
 check('Command menus stay within narrow and short viewports',menu_fit)
 def named_commands():
  for w,h in [(1600,1000),(720,640),(390,844)]:
   page.set_viewport_size({'width':w,'height':h});page.wait_for_timeout(120)
   blank=ev("""()=>[...document.querySelectorAll('.topbar button')].filter(e=>e.getClientRects().length&&getComputedStyle(e).visibility!=='hidden').filter(e=>!e.innerText.trim()&&![...e.querySelectorAll('svg')].some(s=>s.getBoundingClientRect().width>5)).map(e=>e.outerHTML)""")
   expect(not blank,blank)
   expect(page.locator('#moreToolsButton .tool-label').count()==1)
   expect(page.locator('#moreToolsButton').inner_text().strip() in ['More','More tools'])
   expect(page.locator('#moreToolsButton .icon-slot').count()==2,'Resize must not overwrite the icons')
 check('Compact global commands keep visible labels or icons; resizing never duplicates More text',named_commands)
 def focus_toggle():
  menu('#viewMenuButton','Focus preview','menuitemcheckbox');page.locator('#inspectorToggle').click();expect(page.locator('.inspector').is_visible(),'One click should reveal properties from focus view')
  menu('#viewMenuButton','Focus preview','menuitemcheckbox');page.locator('#libraryToggle').click();expect(page.locator('.sidebar').is_visible(),'One click should reveal library from focus view')
  page.set_viewport_size({'width':390,'height':844});page.wait_for_timeout(120);page.locator('#libraryToggle').click();expect(page.locator('.sidebar').is_visible());page.locator('#inspectorToggle').click();expect(page.locator('.inspector').is_visible());expect(not page.locator('.sidebar').is_visible());page.locator('#libraryToggle').click();expect(page.locator('.sidebar').is_visible());expect(not page.locator('.inspector').is_visible())
 check('Focus mode reveals panels in one click and narrow drawers are mutually exclusive',focus_toggle)
 def clear_selection():
  ev("selectMany(['c1','c2'])");before=ev('JSON.stringify(project)');token=ev('documentToken()');page.locator('.timeline-toolbar [data-editor=more]').click();page.get_by_role('menuitem',name='Clear selection',exact=False).click();expect(ev('ui.selected===null && currentIds().length===0'));expect(ev('JSON.stringify(project)')==before);expect(ev('documentToken()')==token)
 check('Clear selection remains pointer-accessible without a permanent Select button',clear_selection)
 check('No JavaScript errors and no network requests during these workflows',lambda:(expect(not errors,errors),expect(not requests,requests)))
 b.close()
(ROOT/'tests/focus-results.json').write_text(json.dumps({'results':results,'errors':errors,'requests':requests},indent=2))
print(json.dumps({'passed':sum(x['status']=='PASS' for x in results),'failed':sum(x['status']=='FAIL' for x in results)},indent=2))
raise SystemExit(any(x['status']=='FAIL' for x in results))
