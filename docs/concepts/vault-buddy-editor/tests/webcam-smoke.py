"""Simulated device permissions/streams; real MediaRecorder, files and project round trip."""
from pathlib import Path
from playwright.sync_api import sync_playwright
import json, zipfile, subprocess
ROOT=Path(__file__).resolve().parents[1]
results=[]

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
with sync_playwright() as p:
    browser=p.chromium.launch(executable_path='/usr/bin/chromium',headless=True,args=['--no-sandbox','--autoplay-policy=no-user-gesture-required'])
    context=browser.new_context(viewport={'width':1440,'height':960},accept_downloads=True)
    page=context.new_page();page.set_default_timeout(7000);errors=[]
    page.on('pageerror',lambda e:errors.append(str(e)))
    page.set_content((ROOT/'vault-buddy-editor.html').read_text());page.wait_for_timeout(700)
    page.evaluate((ROOT/'tests/camera-fixture.js').read_text())
    page.locator('.webcam-entry').click();page.screenshot(path=str(ROOT/'screenshots/03-camera-setup.png'))
    assert page.evaluate("webcamState.phase==='idle'&&!webcamState.stream")
    page.locator('[data-cam="enable"]').click();page.wait_for_function("webcamState.phase==='preview'")
    assert page.evaluate('webcamState.stream.getVideoTracks().length===1&&webcamState.stream.getAudioTracks().length===0')
    page.locator('#webcamMic').check();page.wait_for_function("webcamState.phase==='preview'&&webcamState.stream.getAudioTracks().length===1")
    results.append({'name':'Explicit acquisition and microphone opt-in (simulated devices)','result':'PASS'})
    page.locator('#webcamStart').fill('2');page.locator('[data-cam="record"]').click();page.wait_for_function("webcamState.phase==='recording'")
    page.wait_for_timeout(1400);page.locator('[data-cam="stop"]').click();page.wait_for_function("webcamState.phase==='review'")
    # finishWebcamTake enters 'review' before video.load() asynchronously loads metadata.
    # Wait for that independent readiness contract; retain the file/cleanup assertions.
    page.wait_for_function("Number.isFinite(document.getElementById('webcamVideo').duration)")
    assert page.evaluate("!webcamState.stream&&webcamState.take.file.size>1000&&Number.isFinite(document.getElementById('webcamVideo').duration)&&window.__testTracks.every(t=>t.readyState==='ended')")
    with page.expect_download() as d:page.locator('[data-cam="raw"]').click()
    raw=ROOT/'tests/webcam-recorded.webm';d.value.save_as(str(raw))
    probe=json.loads(subprocess.run(['ffprobe','-v','error','-show_entries','format=duration:stream=codec_name,codec_type,width,height','-of','json',str(raw)],capture_output=True,text=True,check=True).stdout)
    assert any(s['codec_type']=='video' for s in probe['streams']) and any(s['codec_type']=='audio' for s in probe['streams'])
    assert float(probe['format']['duration'])>1
    results.append({'name':'Real recorded WebM decodes with video/audio; own tracks released','result':'PASS','detail':probe})
    page.locator('[data-cam="add"]').click();page.wait_for_function("!document.getElementById('webcamDialog').open")
    assert page.evaluate("project.tracks.length===6&&selectedClip().start_ms===2000&&asset(selectedClip().asset_id).capture_origin==='webcam'")
    page.locator('.precision-section summary').filter(has_text='Frame & crop').click()
    page.locator('[data-video-shape="circle"]').click();page.locator('[data-video-corner="tl"]').click()
    assert page.evaluate("!!validate(project)&&selectedClip().frame_shape==='circle'&&Math.abs(selectedClip().w*1280-selectedClip().h*720)<1")
    clipid=page.evaluate('selectedClip().id')
    results.append({'name':'Add take as independent layer and adjust circular overlay','result':'PASS'})
    with page.expect_download() as d:
        page.locator('.save-project-btn').click();page.locator('[data-action="confirmProjectSave"]').click()
    package=ROOT/'tests/webcam-project.vbproject.zip';d.value.save_as(str(package))
    z=zipfile.ZipFile(package);assert z.testzip() is None and len([n for n in z.namelist() if n.startswith('Sources/')])==1
    assert json.loads(z.read('project.vbproject.json'))['record']['products']==[]
    page.screenshot(path=str(ROOT/'screenshots/04-project-saved.png'))
    fresh=browser.new_context(accept_downloads=True);p2=fresh.new_page();p2.set_content((ROOT/'vault-buddy-editor.html').read_text());p2.wait_for_timeout(300)
    p2.locator('#projectInput').set_input_files(str(package));p2.wait_for_selector('#confirmDialog[open]');p2.locator('#confirmButton').click()
    p2.wait_for_function('id=>!!clip(id)&&!projectBusy',arg=clipid);p2.wait_for_timeout(500)
    assert p2.evaluate("id=>projectHealth().missing.length===0&&projectRecord.products.length===0&&clip(id).frame_shape==='circle'&&runtime(clip(id)).el.readyState>=2",clipid)
    results.append({'name':'Portable project reopens with playable camera take, layout and no render','result':'PASS'})
    assert not errors, errors
    results.append({'name':'No uncaught JavaScript errors during webcam end-to-end flow','result':'PASS'})
    fresh.close();browser.close()
(ROOT/'tests/webcam-results.json').write_text(json.dumps(results,indent=2))
print(json.dumps(results,indent=2))
