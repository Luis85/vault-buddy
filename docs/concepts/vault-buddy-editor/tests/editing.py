"""Tests of the delivered HTML, including user-needs changes. No external network required."""
from pathlib import Path
from playwright.sync_api import sync_playwright
from PIL import Image,ImageDraw
import json,traceback,zipfile,subprocess
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'tests/artifacts';OUT.mkdir(exist_ok=True);RESULTS=[]
def check(name,fn):
 try:
  detail=fn();RESULTS.append({'name':name,'result':'PASS','detail':detail});print('PASS',name,detail or '',flush=True)
 except Exception as e:
  RESULTS.append({'name':name,'result':'FAIL','detail':str(e)});print('FAIL',name,str(e),flush=True);traceback.print_exc()
def expect(v,message='Assertion failed'):
 if not v:raise AssertionError(message)
def ev(p,s):return p.evaluate(s)
def reset(p):
 p.evaluate("""()=>{for(const d of document.querySelectorAll('dialog[open]'))d.close();pause();closeContext();clearRuntime();clearTimeout(saveTimer);project=seed();projectRecord=newProjectRecord(project);history=[];redoStack=[];multiIds=[];clipClipboard=null;lookClipboard=null;pendingRange=null;ui.time=13400;ui.selected={type:'clip',id:'presenter1'};ui.propertyTab='layout';ui.tab='media';ui.zoom=1;ui.snap=true;ui.review=false;ui.mediaSearch='';ui.captionSettings=false;ui.speedBehavior='ripple';productFiles.clear();projectDownloadToken=null;$('#deleteMode').value='gap';$('#timelineScroll').scrollTop=0;$('#timelineScroll').scrollLeft=0;prepareBuiltin();render();syncMedia();} """)
def download(p,fn,path):
 with p.expect_download(timeout=30000) as d:fn()
 d.value.save_as(str(path));return path
image=Image.new('RGB',(600,360),'#2d3840');g=ImageDraw.Draw(image);g.rectangle((20,20,220,170),fill='#f07536');g.ellipse((280,40,550,310),fill='#64ccba');g.text((30,250),'LOCAL IMAGE FIXTURE',fill='white');image.save(ROOT/'tests/fixtures/local-still.png')

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
 context=browser.new_context(viewport={'width':1600,'height':1000},accept_downloads=True);page=context.new_page();page.set_default_timeout(6000);errors=[];network=[]
 page.on('pageerror',lambda e:errors.append(str(e)));page.on('request',lambda r:network.append(r.url) if r.url.startswith(('http:','https:','ws:','wss:')) else None)
 page.set_content((ROOT/'vault-buddy-editor.html').read_text());page.wait_for_timeout(900)
 check('v3 initializes with five tracks, four persistent library tabs and two advisory checks',lambda:(expect(ev(page,"project.schema.endsWith('/3')&&!!validate(project)&&project.tracks.length===5")),expect(page.locator('.sidebar [data-tab]').count()==4),expect(ev(page,'collectIssues().every(i=>i.severity==="note")'))))
 def context_pointer():
  reset(page);page.locator('[data-clip=c2]').click(button='right',position={'x':100,'y':24});t=ev(page,'menuState.target.time');expect(9500<t<23500);page.get_by_role('menuitem',name='Split at',exact=False).click();expect(ev(page,'project.clips.length===8'));expect(abs(ev(page,"clip('c2').out_ms")-(10500+t-9500))<2);expect(ev(page,'history.length===1'));return {'split_ms':t}
 check('Right-click edits the clicked clip at pointer time',context_pointer)
 def keyboard_menu():
  reset(page);page.locator('[data-clip=presenter1]').focus();page.keyboard.press('Shift+F10');expect(page.locator('[role=menu]').count()==1);page.keyboard.press('End');expect('Delete' in ev(page,'document.activeElement.textContent'));page.keyboard.press('Home');expect('Go to' in ev(page,'document.activeElement.textContent'));page.keyboard.type('speed');page.keyboard.press('ArrowRight');expect(page.locator('[role=menu]').count()==2);page.keyboard.press('ArrowLeft');expect(page.locator('[role=menu]').count()==1);page.keyboard.press('Escape');expect(ev(page,"document.activeElement.dataset.clip==='presenter1'"))
 check('Shift+F10, typeahead, arrows, submenu return and restored focus',keyboard_menu)
 def contexts():
  reset(page)
  for selector,kind in [('[data-fx=e-arrow]','effect'),('[data-select-track=v1]','track'),('[data-asset=screen]','asset')]:
   page.locator(selector).first.click(button='right');expect(ev(page,'menuState.target.type')==kind);page.keyboard.press('Escape')
  c=ev(page,"clip('presenter1')");r=page.locator('#preview').bounding_box();page.mouse.click(r['x']+(c['x']+c['w']/2)*r['width'],r['y']+(c['y']+c['h']/2)*r['height'],button='right');expect(ev(page,"menuState.target.id==='presenter1'"));page.keyboard.press('Escape');page.locator('.timeline-toolbar [data-editor=more]').click();expect(page.locator('[role=menu]').count()==1);page.mouse.click(500,80);expect(page.locator('[role=menu]').count()==0)
 check('Object-specific menus plus visible More and outside dismissal',contexts)
 def locked():
  reset(page);ev(page,"toggleTrack('v3','locked')");page.locator('[data-clip=presenter1]').click(button='right');b=page.get_by_role('menuitem',name='Delete · leave gap',exact=True);expect(b.get_attribute('aria-disabled')=='true');b.dispatch_event('click');expect(ev(page,"!!clip('presenter1')"));page.keyboard.press('Escape')
 check('Locked commands expose disabled state and preserve footage',locked)
 def menu_bounds():
  reset(page);page.set_viewport_size({'width':960,'height':640});ev(page,"openContext({type:'clip',id:'c2',time:13000},955,635)");r=page.locator('.context-panel').bounding_box();expect(r['x']+r['width']<=960 and r['y']+r['height']<=640);page.keyboard.press('Escape');page.set_viewport_size({'width':1600,'height':1000});ev(page,"ui.propertyTab='properties';renderInspector()");page.locator('[data-prop=clipName]').click(button='right');expect(page.locator('[role=menu]').count()==0)
 check('Menus stay in viewport; native text menus remain native',menu_bounds)
 def multi_select():
  reset(page);page.locator('[data-clip=c1]').click(position={'x':80,'y':25});page.locator('[data-clip=c2]').click(modifiers=['Shift'],position={'x':100,'y':25});expect(ev(page,'currentIds().length===2'));expect(page.locator('.clip[aria-pressed=true]').count()==2);page.locator('[data-clip=c1]').click(button='right');expect(ev(page,'currentIds().length===2'));expect(page.get_by_role('menuitem',name='Copy selection').count()==1);page.keyboard.press('Escape')
 check('Shift multi-selection survives a right-click',multi_select)
 def group_copy():
  reset(page);ev(page,"selectMany(['c1','c2']);groupSelection();copySelection();pasteClips(40000)");expect(ev(page,'project.clips.length===9&&!!validate(project)&&currentIds().length===2'));expect(ev(page,'multiClips()[1].start_ms-multiClips()[0].start_ms===9500'));expect(ev(page,"multiClips()[0].group_id!==clip('c1').group_id"));ev(page,'undo()');expect(ev(page,'project.clips.length===7'))
 check('Grouped copy/paste remaps attachments and is one undo',group_copy)
 def group_drag():
  reset(page);ev(page,"selectMany(['presenter1','detail1']);groupSelection();ui.snap=false;render()");before=ev(page,"[clip('presenter1').start_ms,clip('detail1').start_ms]");r=page.locator('[data-clip=presenter1]').bounding_box();page.mouse.move(r['x']+100,r['y']+25);page.mouse.down();page.mouse.move(r['x']+160,r['y']+25,steps=8);page.mouse.up();after=ev(page,"[clip('presenter1').start_ms,clip('detail1').start_ms]");expect(after[0]>before[0] and after[0]-before[0]==after[1]-before[1]);expect(ev(page,"clip('presenter1').track_id==='v3'&&clip('detail1').track_id==='v2'&&!!validate(project)"))
 check('Group drag keeps relative timing and original tracks',group_drag)
 def zero_group():
  reset(page);ev(page,"selectMany(['c1','detail1']);groupSelection()");page.locator('[data-clip=c1]').focus();page.keyboard.press('Shift+ArrowLeft');expect(ev(page,"clip('c1').start_ms===0&&clip('detail1').start_ms===11200"))
 check('Group nudge at time zero clamps the group, not individual members',zero_group)
 def transition_group():
  reset(page);ev(page,"selectClip('c1');makeCrossfade(1);window.tx=JSON.stringify(project.transitions);window.positions=JSON.stringify(project.clips.map(c=>c.start_ms));selectMany(['c1','c2']);groupSelection();groupSelection(true)");expect(ev(page,'JSON.stringify(project.transitions)===tx&&JSON.stringify(project.clips.map(c=>c.start_ms))===positions&&!!validate(project)'))
 check('Grouping and ungrouping preserve transitions and alignment',transition_group)
 def batch():
  reset(page);ev(page,"selectMany(['c1','c2'])");page.locator('[data-editor-batch-fade="500"]').click();expect(ev(page,"clip('c1').fade_in_ms===500&&clip('c2').fade_out_ms===500"));page.locator('[data-editor=mute]').click();expect(ev(page,'multiClips().every(c=>c.muted)'));page.locator('[data-editor=unmute]').click();expect(ev(page,'multiClips().every(c=>!c.muted)'))
 check('Batch fade and explicit mute/unmute controls work',batch)
 def speed():
  reset(page);ev(page,"selectClip('c2');applySpeed('c2',2)");expect(ev(page,"clipDuration(clip('c2'))===7000&&clip('c3').start_ms===16500&&clip('presenter1').start_ms===1500"));expect(ev(page,"sourceTime(clip('c2'),10000)===11500&&effective(project.effects.find(e=>e.id==='e-arrow')).output===10000"));ev(page,"seek(12000);split()");expect(ev(page,"clip('c2').out_ms===15500&&!!validate(project)"))
 check('Speed retimes source-linked annotations, split and following clips',speed)
 def speed_guard():
  reset(page);before=ev(page,'JSON.stringify(project)');ev(page,"applySpeed('c2',.25,'leave')");expect(before==ev(page,'JSON.stringify(project)'));ev(page,"selectMany(['c2','detail1']);groupSelection()");expect(not ev(page,"applySpeed('c2',2)"));ev(page,"groupSelection(true);selectClip('c1');makeCrossfade(1)");expect(not ev(page,"applySpeed('c1',2)"))
 check('Speed guard rejects overlaps, grouped retiming and active crossfades',speed_guard)
 def look():
  reset(page);ev(page,"selectClip('presenter1');showProperty('color')");page.locator('[data-editor-filter=mono]').click();el=page.locator('[data-editor-adjust=brightness]');el.evaluate("e=>{e.value='120';e.dispatchEvent(new Event('change',{bubbles:true}))}");expect(ev(page,"clip('presenter1').adjustments.brightness===1.2&&clip('presenter1').adjustments.grayscale===1"));ev(page,"copyLook();selectClip('c2');pasteLook()");expect(ev(page,"clip('c2').adjustments.grayscale===1&&clip('c2').in_ms===10500"));ev(page,"transformClip('c2','rotate');transformClip('c2','flip-y');transformClip('c2','flip')");expect(ev(page,"clip('c2').rotation===90&&clip('c2').flip_y&&clip('c2').mirror"))
 check('Color controls, look paste, rotation and flips preserve source timing',look)
 def canvas_formats():
  reset(page)
  for key,w,h in [('portrait',720,1280),('square',720,720),('classic',960,720),('wide',1280,720)]:
   ev(page,f"canvasFormat('{key}')");expect(ev(page,'[canvas.width,canvas.height]')==[w,h]);expect(ev(page,"Math.abs(clip('presenter1').w*W-clip('presenter1').h*H)<1&&!!validate(project)"))
  page.locator('#ratioButton').click();expect(page.locator('.canvas-choice').count()==4);page.keyboard.press('Escape')
 check('Aspect ratio changes real output size and keeps circular webcam geometry',canvas_formats)
 def intro():
  reset(page);before=ev(page,'project.clips.map(c=>({id:c.id,start:c.start_ms,track:c.track_id}))');ev(page,"addTitleCard('intro',0,null,true)");expect(ev(page,'project.clips.length===8&&!!validate(project)'))
  for x in before:expect(ev(page,f"clip('{x['id']}').start_ms==={x['start']+5000}&&clip('{x['id']}').track_id==='{x['track']}'"))
  ev(page,'undo()');expect(ev(page,"clip('c1').start_ms===0&&project.clips.length===7"));ev(page,"toggleTrack('a2','locked');window.before=JSON.stringify(project);addTitleCard('intro',0,null,true)");expect(ev(page,'JSON.stringify(project)===before'))
 check('Safe intro insertion shifts every track atomically and respects locks',intro)
 def cards():
  reset(page);page.locator('[data-tab=create]').click();page.locator('[data-editor-card=chapter]').click();page.fill('[data-editor-card-prop=title]','One <safe> title');page.locator('[data-editor-card-prop=title]').dispatch_event('change');expect(ev(page,"selectedClip().card.title==='One <safe> title'&&!!validate(project)"));page.fill('[data-editor-card-duration]','3');page.locator('[data-editor-card-duration]').dispatch_event('change');expect(ev(page,'clipDuration(selectedClip())===3000'))
 check('Local cards provide editable text and duration',cards)
 def still():
  reset(page);page.locator('#mediaInput').set_input_files(str(ROOT/'tests/fixtures/local-still.png'));page.wait_for_function("project.assets.some(a=>a.media_type==='image')");ev(page,"addAsset(project.assets.find(a=>a.media_type==='image').id)");expect(ev(page,"clipDuration(selectedClip())===5000&&!!media.get(selectedClip().asset_id).image&&!!validate(project)"));page.fill('#mediaSearch','still');expect(page.locator('.media-card:visible').count()==1)
 check('Real still image import and media search',still)
 def caption_import():
  reset(page);ev(page,"selectClip('c2');importCaptionsText('1\\n00:00:01,000 --> 00:00:03,000\\nGuten Tag — project name\\n\\n2\\n00:00:04,000 --> 00:00:06,000\\nClick Create.', 'c2')");expect(ev(page,'captionView()[0].output===10500'));ev(page,"applySpeed('c2',2)");expect(ev(page,'captionView()[0].output===10000&&captionView()[0].duration===1000'));expect('00:00:10,000 --> 00:00:11,000' in ev(page,'exportSRT()'));v=ev(page,"parseSubtitles('WEBVTT\\n\\n00:01.000 --> 00:03.000 align:start\\n<b>First</b> &amp; next')");expect(v[0]['text']=='First & next')
 check('SRT/VTT parsing preserves text and maps source-linked caption speed',caption_import)
 def caption_edit():
  reset(page);ev(page,"selectClip('c2');seek(13000)");page.locator('[data-tab=captions]').click();page.locator('[data-editor=caption-add]').click();page.fill('#captionText','Explain the next click.');page.fill('#captionEnd','1');page.locator('#captionForm button[type=submit]').click();expect(page.locator('#captionDialog').is_visible());expect(bool(page.locator('#captionError').inner_text()));page.fill('#captionStart','2');page.fill('#captionEnd','5');page.locator('#captionForm button[type=submit]').click();expect(ev(page,'captionView()[0].output===11500'));page.locator('.caption-settings summary').click();page.locator('[data-cap-setting=burn_in]').uncheck();expect(ev(page,'!captionConfig().burn_in&&ui.captionSettings'));expect(page.locator('.caption-cue').count()==1)
 check('Caption dialog validates timing and preserves open appearance controls',caption_edit)
 def caption_splits():
  reset(page);ev(page,"selectClip('c2');importCaptionsText('1\\n00:00:01,000 --> 00:00:06,000\\nA source linked caption that should follow the video', 'c2');window.id=captionConfig().cues[0].id;window.d=captionView()[0].duration;splitCaption(id)");expect(ev(page,'captionView().length===2&&captionView().reduce((n,q)=>n+q.duration,0)===d'));ev(page,'undo();seek(13000);split()');expect(ev(page,'captionView().length===2&&captionView().reduce((n,q)=>n+q.duration,0)===5000'))
 check('Caption text split and clip split conserve timing',caption_splits)
 def checks():
  reset(page);ev(page,"selectClip('c1');deleteSelection();showIssues()");expect('No video' in page.locator('#issuesBody').inner_text());page.get_by_role('button',name='Close checks',exact=True).click();ev(page,"toggleTrack('a1','solo');showIssues()");expect('Audio solo is enabled' in page.locator('#issuesBody').inner_text());ev(page,"document.querySelector('#issuesDialog').close();selectClip('c2');importCaptionsText('1\\n00:00:00,100 --> 00:00:00,300\\nThis caption is much too long to read in a fraction of a second.', 'c2');showIssues()");expect('hard to read' in page.locator('#issuesBody').inner_text())
 check('Actionable checks flag video gaps, audio solo and hard-to-read captions',checks)
 def caption_collision():
  reset(page);ev(page,"importCaptionsText('1\\n00:00:01,000 --> 00:00:05,000\\nRead this instruction', 'c2');showIssues()");expect('share the lower area' in page.locator('#issuesBody').inner_text());page.get_by_role('button',name='Move captions to top').click();expect(ev(page,"captionConfig().position==='top'&&!collectIssues().some(i=>i.id==='caption-overlay')"))
 check('Caption/callout collision has a working corrective action',caption_collision)
 def privacy():
  reset(page);ev(page,"selectClip('c2');seek(13000);addPrivacyCover()");expect(ev(page,"project.effects.at(-1).kind==='mask'&&project.effects.at(-1).start_ms===clip('c2').in_ms&&project.effects.at(-1).end_ms===clip('c2').out_ms&&!!validate(project)"));ev(page,'downloadProject()');expect('uncensored originals' in page.locator('#projectSaveBody').inner_text());page.keyboard.press('Escape');ev(page,'undo()');expect(ev(page,"!project.effects.some(e=>e.kind==='mask')"))
 check('Opaque privacy cover is undoable and warns about originals in project packages',privacy)
 def clear_save():
  reset(page);expect('Save project needed' in page.locator('#saveStatus').inner_text());f=download(page,lambda:ev(page,"downloadProject();document.querySelector('[name=projectFormat][value=reference]').checked=true;confirmProjectSave()"),OUT/'project-save-label.vbproject.json');expect('download requested' in page.locator('#saveStatus').inner_text());ev(page,"change(()=>project.title='Unsaved name','Rename')");expect('Save project needed' in page.locator('#saveStatus').inner_text());expect(ev(page,"(()=>{const e=new Event('beforeunload',{cancelable:true});window.dispatchEvent(e);return e.defaultPrevented;})()"))
 check('Save status distinguishes download request from dirty edits and cache',clear_save)
 def gap_guards():
  reset(page);ev(page,"closeGaps('v2')");expect(ev(page,"clip('detail1').start_ms===0&&clip('presenter1').start_ms===1500"));ev(page,"undo();selectMany(['detail1','presenter1']);groupSelection();closeGaps('v2')");expect(ev(page,"clip('detail1').start_ms===11200"))
 check('Gap cleanup does not silently break cross-track groups',gap_guards)
 def navigation():
  reset(page);ev(page,"selectClip('c2');seek(0);goToSelection(true)");expect(ev(page,"ui.time>clip('c2').start_ms&&ui.time<end(clip('c2'))&&ui.zoom>1"));expect(ev(page,"document.activeElement.dataset.clip==='c2'"))
 check('Go to and fit selection connect the timeline with its preview',navigation)
 def schema():
  reset(page)
  for m in ["p.clips[0].speed=0","p.clips[0].rotation=45","p.clips[0].adjustments={brightness:Infinity}","p.captions={}","p.canvas.width=999"]:expect(ev(page,f"(()=>{{const p=deep(project);{m};try{{validate(p);return false}}catch{{return true}}}})()"),m)
  expect(ev(page,"(()=>{const p=deep(project);p.schema='vault-buddy-video-project/2';return validate(migrateV1(p)).schema.endsWith('/3')})()"))
 check('Strict extended schema and v2 migration',schema)
 def workspace():
  reset(page);ev(page,"selectClip('c2');applySpeed('c2',2);setTreatment(['c2'],'warm');importCaptionsText('1\\n00:00:00,500 --> 00:00:01,500\\nRound trip cue', 'c2');selectMany(['c1','c2']);groupSelection();canvasFormat('square');ui.tab='captions';ui.captionSettings=true;render()");original=ev(page,'JSON.stringify(project)');f=download(page,lambda:ev(page,"downloadProject();document.querySelector('[name=projectFormat][value=reference]').checked=true;confirmProjectSave()"),OUT/'workspace.vbproject.json');ev(page,"project.title='Other';render()");page.locator('#projectInput').set_input_files(str(f));page.wait_for_selector('#confirmDialog[open]');page.locator('#confirmButton').click();page.wait_for_function('!projectBusy&&project.title!=="Other"');expect(ev(page,'JSON.stringify(project)')==original);expect(ev(page,'currentIds().length===2&&ui.captionSettings&&W===720&&H===720'))
 check('Workspace reopens with speed, color, captions, group selection and format',workspace)
 def recovery():
  reset(page);page.locator('#mediaInput').set_input_files([str(ROOT/'tests/fixtures/capture-with-audio.mp4'),str(ROOT/'tests/fixtures/narration-tone.wav')]);page.wait_for_function('!ui.importing&&project.assets.length===7');ev(page,"window.imported=project.assets.filter(a=>!a.builtin);window.relinkFiles=imported.map(a=>media.get(a.id).file);for(const a of imported)addAsset(a.id);window.positions=JSON.stringify(project.clips);for(const a of imported)media.delete(a.id);clearRuntime();render();showRelinkDialog()");expect('Your edit is still here' in page.locator('#relinkBody').inner_text());result=ev(page,'batchRelink(relinkFiles)');expect(sum(s.endswith(': reconnected') for s in result)==2);expect(ev(page,'projectHealth().missing.length===0&&JSON.stringify(project.clips)===positions'))
 check('Batch source reconnection restores media without changing the edit',recovery)
 def ambiguous_recovery():
  reset(page);page.locator('#mediaInput').set_input_files(str(ROOT/'tests/fixtures/capture-with-audio.mp4'));page.wait_for_function('!ui.importing&&project.assets.length===6');ev(page,"window.a=project.assets.find(a=>!a.builtin);window.file=media.get(a.id).file;media.delete(a.id)");result=ev(page,'batchRelink([file,file])');expect('ambiguous' in result[0]);expect(ev(page,'!media.has(a.id)'))
 check('Ambiguous reconnect candidates are not chosen automatically',ambiguous_recovery)
 def portable_image():
  reset(page);page.locator('#mediaInput').set_input_files(str(ROOT/'tests/fixtures/local-still.png'));page.wait_for_function("!ui.importing&&project.assets.some(a=>a.media_type==='image')");ev(page,"addAsset(project.assets.find(a=>a.media_type==='image').id)");id=ev(page,'selectedClip().asset_id');f=download(page,lambda:ev(page,'downloadProject();confirmProjectSave()'),OUT/'still-project.vbproject.zip');
  with zipfile.ZipFile(f) as z:expect(z.testzip() is None);expect(any(n.endswith('.png') for n in z.namelist()))
  ev(page,"project.title='Other';render()");page.locator('#projectInput').set_input_files(str(f));page.wait_for_selector('#confirmDialog[open]');page.locator('#confirmButton').click();page.wait_for_function('!projectBusy&&project.title!=="Other"');expect(ev(page,f"!!media.get('{id}').image&&!!validate(project)"))
 check('Portable image project includes real bytes and reopens them',portable_image)
 def range_metadata():
  reset(page);ev(page,"openSave()");page.locator('#renderRangeEnabled').check();page.fill('#renderRangeStart','3');page.fill('#renderRangeEnd','1');page.locator('#renderRangeEnd').dispatch_event('change');expect(page.locator('#saveFooter [data-action=downloadPackage]').is_disabled());page.fill('#renderRangeStart','1');page.fill('#renderRangeEnd','3');page.locator('#renderRangeEnd').dispatch_event('change');expect(not page.locator('#saveFooter [data-action=downloadPackage]').is_disabled());expect(ev(page,'chosenRenderRange()')=={'start_ms':1000,'end_ms':3000})
 check('Review-range controls reject invalid bounds and allow a short range',range_metadata)
 def diagnostics():
  reset(page);f=download(page,lambda:ev(page,'diagnosticReport()'),OUT/'diagnostics.json');s=f.read_text();expect('Create your first project' not in s and 'Presenter' not in s and 'capture-with-audio.mp4' not in s);expect(json.loads(s)['counts']['tracks']==5)
 check('Support diagnostics omit names, paths, captions, frames and audio',diagnostics)
 def responsive():
  reset(page)
  for w,h in [(1600,1000),(1280,800),(960,640),(720,480),(390,844)]:
   page.set_viewport_size({'width':w,'height':h});page.wait_for_timeout(150);expect(ev(page,'document.body.scrollWidth<=innerWidth+1'),f'overflow {w}');expect(ev(page,'canvas.getBoundingClientRect().height>40'));page.screenshot(path=str(ROOT/f'screenshots/compact-{w}.png'))
  page.set_viewport_size({'width':1600,'height':1000})
 check('Five desktop/compact layouts retain preview and avoid page overflow',responsive)
 check('No unhandled browser exceptions',lambda:expect(not errors,str(errors)))
 check('No external media, font or API requests',lambda:expect(not network,str(network)))
 (ROOT/'tests/editing-results.json').write_text(json.dumps({'scenarios':RESULTS,'browser_errors':errors,'network_requests':network},indent=2));browser.close()
print('RESULT',sum(x['result']=='PASS' for x in RESULTS),'/',len(RESULTS),flush=True)
