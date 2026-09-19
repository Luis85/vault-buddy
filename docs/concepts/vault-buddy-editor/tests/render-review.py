"""Produce and decode a real non-zero range, then review the actual product file."""
from pathlib import Path
from playwright.sync_api import sync_playwright
import json,subprocess,zipfile,traceback,math,array
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'tests/artifacts';OUT.mkdir(exist_ok=True);results=[]
def test(name,fn):
 try:
  detail=fn();results.append({'name':name,'result':'PASS','detail':detail});print('PASS',name,detail or '',flush=True)
 except Exception as e:
  blocked=name.startswith('Origin-backed') and 'ERR_BLOCKED_BY_ADMINISTRATOR' in str(e);state='BLOCKED' if blocked else 'FAIL';results.append({'name':name,'result':state,'detail':str(e)});print(state,name,str(e),flush=True)
  if not blocked:traceback.print_exc()
def req(v,msg='Assertion failed'):
 if not v:raise AssertionError(msg)
html=(ROOT/'vault-buddy-editor.html').read_text()

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
 b=pw.chromium.launch(executable_path='/usr/bin/chromium',headless=True,args=['--no-sandbox','--autoplay-policy=no-user-gesture-required']);ctx=b.new_context(viewport={'width':1600,'height':1000},accept_downloads=True);p=ctx.new_page();p.set_default_timeout(8000);errors=[];p.on('pageerror',lambda e:errors.append(str(e)));p.set_content(html);p.wait_for_timeout(700)
 p.locator('#mediaInput').set_input_files([str(ROOT/'tests/fixtures/capture-with-audio.mp4'),str(ROOT/'tests/fixtures/narration-tone.wav'),str(ROOT/'tests/fixtures/local-still.png')]);p.wait_for_function('!ui.importing&&project.assets.some(a=>a.media_type==="image")')
 def prepare():
  p.evaluate("""()=>{pause();clearRuntime();const v=project.assets.find(a=>a.name==='capture-with-audio.mp4'),audio=project.assets.find(a=>a.name==='narration-tone.wav'),image=project.assets.find(a=>a.media_type==='image');project.assets=[v,audio,image];project.effects=[];project.markers=[];project.transitions=[];project.tracks=project.tracks.filter(t=>['v1','v2','a1'].includes(t.id));project.tracks.forEach(t=>{t.muted=false;t.volume=.6});project.clips=[newClip('motion',v.id,'v1','Screen',0,0,6000,{speed:2,rotation:90}),newClip('narration',audio.id,'a1','Narration',0,0,6000,{speed:2}),newClip('still',image.id,'v2','Image',0,0,3000,{x:.52,y:.06,w:.42,h:.2,muted:true})];project.title='Range review verification';projectRecord=newProjectRecord(project);ui.selected={type:'clip',id:'motion'};multiIds=['motion'];canvasFormat('portrait');setTreatment(['motion'],'mono');importCaptionsText('1\\n00:00:00,200 --> 00:00:02,500\\nCheck the actual rendered file.', 'motion');seek(1300);render();syncMedia(true);validate(project);} """)
  req(p.evaluate('duration()===3000&&W===720&&H===1280&&captionView().length===1'));return {'source_ms':6000,'edited_ms':3000,'size':[720,1280]}
 test('Build two-speed video/audio, portrait framing, still layer and captions',prepare)
 def pixels():
  difference=p.evaluate("""()=>{const c=document.createElement('canvas');c.width=W;c.height=H;const g=c.getContext('2d');paint(g,false);const a=g.getImageData(0,H*.7,W,H*.3).data;project.captions.burn_in=false;paint(g,false);const z=g.getImageData(0,H*.7,W,H*.3).data;let n=0;for(let i=0;i<a.length;i+=4)if(a[i]!==z[i]||a[i+1]!==z[i+1])n++;project.captions.burn_in=true;return n;}""");req(difference>100);return {'changed_pixels':difference}
 test('Burn-in switch changes composed pixels, not merely metadata',pixels)
 def render():
  p.evaluate('window.beforeRender=JSON.stringify(project);openSave()');p.locator('#renderRangeEnabled').check();p.fill('#renderRangeStart','1');p.fill('#renderRangeEnd','2.6');p.locator('#renderRangeEnd').dispatch_event('change');req('00:01.6' in p.locator('#saveBody .render-profile .small').inner_text())
  with p.expect_download(timeout=30000) as event:p.locator('#saveFooter [data-action=downloadPackage]').click()
  event.value.save_as(str(OUT/'review-range.zip'))
  with zipfile.ZipFile(OUT/'review-range.zip') as z:
   req(z.testzip() is None);video=next(n for n in z.namelist() if n.endswith(('.webm','.mp4')));(OUT/'review-range.webm').write_bytes(z.read(video));srt=z.read(next(n for n in z.namelist() if n.endswith('.srt'))).decode();req('00:00:00,000 --> 00:00:01,500' in srt,srt);product=json.loads(z.read('Render/product.json'));req(product['duration_ms']==1600 and product['render_range']=={'start_ms':1000,'end_ms':2600});req(product['snapshot']['clips'][0]['speed']==2)
  req(p.evaluate('JSON.stringify(project)===beforeRender'));return {'range_ms':[1000,2600],'bytes':(OUT/'review-range.zip').stat().st_size}
 test('Non-zero review range rebases captions and leaves project untrimmed',render)
 def decode():
  probe=json.loads(subprocess.run(['ffprobe','-v','error','-show_entries','format=duration:stream=codec_name,codec_type,width,height','-of','json',str(OUT/'review-range.webm')],capture_output=True,text=True,check=True).stdout);video=next(s for s in probe['streams'] if s['codec_type']=='video');req(video['width']==720 and video['height']==1280);req(any(s['codec_type']=='audio' for s in probe['streams']));req(abs(float(probe['format']['duration'])-1.6)<.2,probe);subprocess.run(['ffmpeg','-v','error','-ss','0.7','-i',str(OUT/'review-range.webm'),'-frames:v','1','-y',str(OUT/'review-range-frame.png')],check=True);req((OUT/'review-range-frame.png').is_file(),'Decoder must emit a later video frame, not just exit successfully.');return probe
 test('Encoded video and audio decode at the intended size and duration',decode)
 def motion_coverage():
  frames=json.loads(subprocess.run(['ffprobe','-v','error','-select_streams','v:0','-show_entries','frame=pts_time','-of','json',str(OUT/'review-range.webm')],capture_output=True,text=True,check=True).stdout)['frames']
  times=[float(f['pts_time']) for f in frames]
  req(len(times)>=4,{'frames':len(times),'times':times})
  req(times[0]<.2 and any(.4<=t<=1.15 for t in times) and times[-1]>=1.15,{'times':times})
  req(times[-1]<=1.9,{'last_frame_s':times[-1],'expected_range_s':1.6})
  paths=[]
  for label,n in [('first',0),('last',len(times)-1)]:
   path=OUT/('review-range-'+label+'.png');paths.append(path)
   subprocess.run(['ffmpeg','-v','error','-i',str(OUT/'review-range.webm'),'-vf',f"select=eq(n\\,{n})",'-vsync','0','-frames:v','1','-y',str(path)],check=True)
   req(path.is_file(),'Expected an actual decoded frame image.')
  from PIL import Image,ImageChops
  first,last=[Image.open(p).convert('RGB') for p in paths];difference=ImageChops.difference(first,last)
  raw=difference.tobytes();changed=sum(bool(raw[i] or raw[i+1] or raw[i+2]) for i in range(0,len(raw),3));req(changed>1000,{'changed_pixels':changed})
  return {'decoded_frames':len(times),'timestamps_s':times,'first_to_last_changed_pixels':changed,'meaning':'Temporal/pixel coverage for this fixture, not a 30 fps or frame-accuracy certificate.'}
 test('Decoded motion covers the beginning, middle and end of the requested range',motion_coverage)
 def audio():
  raw=subprocess.run(['ffmpeg','-v','error','-i',str(OUT/'review-range.webm'),'-vn','-ac','1','-ar','24000','-f','f32le','-'],capture_output=True,check=True).stdout;data=array.array('f');data.frombytes(raw);rms=math.sqrt(sum(x*x for x in data)/len(data));req(rms>.001,rms);return {'audio_rms':rms,'samples':len(data)}
 test('Encoded range contains audible mixed audio',audio)
 def watch():
  p.locator('#saveFooter [data-watch-product]').click();p.wait_for_function("document.querySelector('#renderedVideo').readyState>=2");req(p.evaluate("document.querySelector('#renderedVideo').src.startsWith('blob:')&&Number.isFinite(document.querySelector('#renderedVideo').duration)"));p.evaluate("document.querySelector('#renderedVideo').play()");p.wait_for_timeout(400);req(p.evaluate("document.querySelector('#renderedVideo').currentTime>.15"));p.screenshot(path=str(ROOT/'screenshots/11-actual-render-review.png'));p.get_by_role('button',name='Close rendered video',exact=True).click();p.wait_for_function('productPreviewURL===null');req(p.evaluate('productPreviewURL===null'));return {'verified':'Actual encoded Blob played, with finite duration; URL released on close.'}
 test('Watch rendered file uses the encoded product and releases its media URL',watch)
 def immutable():
  p.evaluate("window.snapshot=JSON.stringify(ensureRecord().products[0].snapshot);canvasFormat('wide');setTreatment(['motion'],'warm')");req(p.evaluate('JSON.stringify(ensureRecord().products[0].snapshot)===snapshot&&ensureRecord().products[0].duration_ms===1600'));p.evaluate('validateWorkspace(workspaceEnvelope())')
 test('Later framing and color edits preserve the earlier product snapshot',immutable)
 def recovery():
  fresh=b.new_context();q=fresh.new_page();q.route('https://vault-buddy.test/**',lambda route:route.fulfill(status=200,content_type='text/html',body=html));q.goto('https://vault-buddy.test/editor');q.wait_for_function('!!window.VaultBuddyContext');q.evaluate("change(()=>project.title='Recovery test','Rename');persistRecovery()");q.reload();q.wait_for_function("project.title==='Recovery test'");fresh.close()
 test('Origin-backed cache reload (requires permitted origin navigation)',recovery)
 test('No unhandled browser errors in the output review lifecycle',lambda:req(not errors,str(errors)))
 b.close()
(ROOT/'tests/render-review-results.json').write_text(json.dumps(results,indent=2));print('TOTAL',len(results),'PASS',sum(x['result']=='PASS' for x in results),'BLOCKED',sum(x['result']=='BLOCKED' for x in results),'FAIL',sum(x['result']=='FAIL' for x in results))
