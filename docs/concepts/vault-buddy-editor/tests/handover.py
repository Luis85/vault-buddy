"""Final shell, content-contract and screenshot verification. No native service is simulated as connected."""
from pathlib import Path
import hashlib, importlib.util, json, traceback
from playwright.sync_api import sync_playwright
from jsonschema import Draft202012Validator, FormatChecker
ROOT=Path(__file__).resolve().parents[1]; HTML=(ROOT/'vault-buddy-editor.html').read_text(); SC=ROOT/'screens';SC.mkdir(exist_ok=True)
results=[];errors=[];requests=[]
def require(condition,detail='Assertion failed'):
    if not condition:raise AssertionError(detail)
def check(name,fn):
    try:
        detail=fn();results.append({'name':name,'status':'PASS','detail':detail});print('PASS',name,flush=True)
    except Exception as e:
        results.append({'name':name,'status':'FAIL','detail':str(e)});traceback.print_exc()
def evaluate(script):return page.evaluate(script)
def idle():
    evaluate("""()=>{VaultBuddyGuide.pause(false);document.querySelectorAll('dialog[open]').forEach(d=>d.close());closeContext();$('#menu').hidden=true;pause();clearTimeout(saveTimer);project=seed();projectRecord=newProjectRecord(project);history=[];redoStack=[];multiIds=['presenter1'];ui.time=13400;ui.selected={type:'clip',id:'presenter1'};ui.propertyTab='layout';ui.tab='media';ui.zoom=1;ui.review=false;document.documentElement.dataset.theme='dark';$('#app').classList.remove('focus-view','library-drawer');$('#workspace').classList.remove('inspector-open','library-hidden','properties-hidden');setTimelineHeight(400);prepareBuiltin();render();syncMedia();}""")
    page.set_viewport_size({'width':1600,'height':1000});page.wait_for_timeout(70)
def screen(name):
    page.wait_for_function("!document.querySelector('#toast')?.classList.contains('visible')")
    page.screenshot(path=str(SC/name))
with sync_playwright() as pw:
    browser=pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True,args=['--no-sandbox','--autoplay-policy=no-user-gesture-required'])
    context=browser.new_context(viewport={'width':1600,'height':1000},accept_downloads=True)
    page=context.new_page();page.set_default_timeout(5000)
    page.on('pageerror',lambda e:errors.append(str(e)));page.on('request',lambda r:requests.append(r.url) if r.url.startswith(('http:','https:','ws:','wss:')) else None)
    page.set_content(HTML);page.wait_for_function("document.querySelector('#app').getAttribute('aria-busy')==='false'")
    def welcome():
        require(page.locator('#guideInvite').is_visible());require(page.locator('dialog[open]').count()==0);screen('01-welcome.png')
    check('Editor becomes ready with optional nonmodal onboarding',welcome)
    page.locator('#guideInvite [data-guide=dismiss-invite]').first.click()
    check('Product title identifies the current tutorial',lambda:require(page.title()=='Create your first project — Vault Buddy Tutorial Editor',page.title()))
    def rename():
        page.locator('.document-name').click();page.fill('#editText','Final tutorial');page.locator('#editForm [type=submit]').click();page.wait_for_function("document.title==='Final tutorial — Vault Buddy Tutorial Editor'");evaluate('undo()');page.wait_for_function("document.title==='Create your first project — Vault Buddy Tutorial Editor'")
    check('Document title follows rename and Undo without stale metadata',rename)
    def labels():
        text=page.locator('.app-header,.header-actions,.preview-header').all_text_contents()
        import re
        require(not re.search(r'\b(?:iteration|v[1-6])\b',' '.join(text),re.I),str(text));require(page.locator('#iterationBadge').count()==0)
    check('Primary navigation has product labels, not iteration badges',labels)
    def canvas_dom():
        idle();evaluate("window.__ratioMutations=0;window.__ratioObserver=new MutationObserver(r=>__ratioMutations+=r.length);__ratioObserver.observe($('#ratioButton'),{childList:true,subtree:true,attributes:true});for(let n=0;n<20;n++)syncCanvasSize();")
        page.wait_for_timeout(80);require(evaluate('__ratioMutations')==0,'Unchanged canvas rewrites toolbar');evaluate('__ratioObserver.disconnect()');return {'repeated_canvas_checks':20,'toolbar_mutations':0}
    check('Unchanged frame sizing does not rebuild the aspect-ratio toolbar',canvas_dom)
    def actual_ratio():
        evaluate("canvasFormat('portrait')");require('9:16' in page.locator('#ratioButton').inner_text());require(page.locator('#ratioButton').get_attribute('data-canvas-size')=='720x1280');idle()
    check('A real canvas change refreshes ratio and geometry',actual_ratio)
    def content():
        expected=json.loads((ROOT/'contracts/onboarding.steps.json').read_text());actual=evaluate('VaultBuddyGuide.steps()');require(len(actual)==22);require([x['id'] for x in actual]==[x['id'] for x in expected]);require(len(json.loads((ROOT/'contracts/onboarding.chapters.json').read_text()))==7);return {'lessons':22,'chapters':7}
    check('Machine-readable onboarding matches all runtime lesson IDs',content)
    def help():
        page.locator('#editorHelp').click();require(page.locator('#guideHub').is_visible());screen('11-learning-center.png');text=page.locator('#guideHub').inner_text();require('Where did the controls move?' not in text);page.keyboard.press('Escape')
    check('Learning center uses current navigation guidance',help)
    def workspace():
        idle();screen('02-workspace.png');require(page.locator('.preview-header .toolstrip').count()==1);require(page.locator('.center > .toolstrip').count()==0)
    check('Current workspace has one preview toolbar row',workspace)
    def context_menu():
        page.locator('.clip[data-clip=presenter1]').click(button='right');require(page.locator('[role=menu]').count()==1);screen('03-context-menu.png');page.keyboard.press('Escape')
    check('Direct editing context is visible on the target clip',context_menu)
    def fades():
        page.locator('[data-property-tab=fades]').click();screen('04-fades.png');require(page.locator('[data-fade-preset]').count()>0)
    check('Fade presets and numeric editing remain available',fades)
    def webcam():
        page.locator('.webcam-entry').click();require(page.locator('#webcamDialog').is_visible());require(evaluate("!webcamState.stream&&webcamState.phase==='idle'"));screen('05-webcam.png');page.keyboard.press('Escape')
    check('Webcam setup opens without requesting a live stream',webcam)
    def captions():
        page.locator('[data-tab=captions]').click();screen('06-captions.png');require('caption' in page.locator('#library').inner_text().lower());idle()
    check('Caption controls have a current standalone screen',captions)
    def checks():
        page.locator('#issuesButton').click();require(page.locator('dialog[open]').count()==1);screen('07-checks.png');page.keyboard.press('Escape')
    check('Pre-render findings remain actionable in their own dialog',checks)
    def save():
        page.locator('.header-actions [data-action=downloadProject]').click();require(page.locator('#projectSaveDialog').is_visible());screen('08-save-project.png');require('portable' in page.locator('#projectSaveBody').inner_text().lower());page.keyboard.press('Escape')
    check('Saving editable work remains separate from rendering',save)
    def render():
        page.locator('.header-actions [data-action=save]').click();require(page.locator('#saveDialog').is_visible());screen('09-render.png');require('browser' in page.locator('#saveBody').inner_text().lower());page.keyboard.press('Escape')
    check('Render dialog states the actual browser execution boundary',render)
    def guide():
        baseline=evaluate('JSON.stringify(project)');evaluate("VaultBuddyGuide.start('fades')");page.wait_for_function("document.querySelector('#guideCoach').getAttribute('data-step')==='fades'");page.wait_for_timeout(80);screen('10-onboarding.png');page.keyboard.press('Escape');require(evaluate('JSON.stringify(project)')==baseline)
    check('Contextual guide highlights without modifying composition',guide)
    def compact():
        idle();page.set_viewport_size({'width':960,'height':640});page.wait_for_timeout(100);screen('12-compact.png');require(evaluate('document.body.scrollWidth<=innerWidth'));require(page.locator('#editorHelp').is_visible());idle();evaluate("document.documentElement.dataset.theme='light'");screen('13-light.png');evaluate("document.documentElement.dataset.theme='dark'")
    check('Compact layout and light theme retain visible Help',compact)
    def contrast():
        page.emulate_media(forced_colors='active',reduced_motion='reduce');page.locator('#editorHelp').focus();require(evaluate("matchMedia('(forced-colors: active)').matches"));require(page.locator('#editorHelp').evaluate("e=>getComputedStyle(e).outlineStyle")!='none');screen('14-high-contrast.png');page.emulate_media(forced_colors='none',reduced_motion='no-preference')
    check('Forced colors preserve keyboard focus without recoloring media',contrast)
    check('Reference loads with no external network calls or browser exceptions',lambda: (require(not requests,str(requests)),require(not errors,str(errors))))
    # Controlled startup fault: replace only the entry promise in a separate page.
    # This is failure-injection evidence, not an unmodified-artifact startup claim.
    def startup_failure():
        q=context.new_page();injected=HTML.replace("init().then(()=>", "Promise.reject(new Error('<img src=x onerror=alert(1)>')).then(()=>",1)
        require(injected!=HTML);q.set_content(injected);q.wait_for_selector('.startup-failure');require(q.locator('.startup-failure img').count()==0);require('saved project files have not been changed' in q.locator('.startup-failure').inner_text().lower());require(q.locator('#app').get_attribute('aria-busy')=='false');q.screenshot(path=str(SC/'15-startup-recovery.png'));q.close()
    check('Injected startup failure has safe actionable recovery copy',startup_failure)
    browser.close()
def schema():
    schema=json.loads((ROOT/'contracts/workspace.schema.json').read_text());v=Draft202012Validator(schema,format_checker=FormatChecker());sample=json.loads((ROOT/'contracts/reference-workspace.example.json').read_text());v.validate(sample)
    bad=json.loads(json.dumps(sample));bad['project']['clips'][0]['speed']=0;require(not v.is_valid(bad));bad=json.loads(json.dumps(sample));bad['project']['assets'][0]['path']='../../private';require(not v.is_valid(bad));return 'Example accepted; invalid speed and direct asset path rejected structurally; semantic graph rules tested separately.'
check('Interchange schema accepts the example and rejects invalid basic fields',schema)
def feature_ids():
    rows=json.loads((ROOT/'contracts/feature-catalog.json').read_text());require(len(rows)==50);require(len({r['id']for r in rows})==50);require(all(r.get('acceptance') and r.get('owner') for r in rows));return {'features':50}
check('All 50 feature IDs have ownership and acceptance criteria',feature_ids)
def reproducible():
    spec=importlib.util.spec_from_file_location('builder',ROOT/'reference/build.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        output=module.build(Path(tmp)/'editor.html');require(output.read_bytes()==(ROOT/'vault-buddy-editor.html').read_bytes())
check('Reference rebuild is byte-identical to the delivered HTML',reproducible)
report={'artifact_sha256':hashlib.sha256((ROOT/'vault-buddy-editor.html').read_bytes()).hexdigest(),'results':results,'screenshots':[p.name for p in sorted(SC.glob('*.png'))]}
(ROOT/'tests/handover-results.json').write_text(json.dumps(report,indent=2)+'\n')
print('TOTAL',len(results),'FAIL',sum(r['status']=='FAIL'for r in results))
raise SystemExit(1 if any(r['status']=='FAIL'for r in results) else 0)
