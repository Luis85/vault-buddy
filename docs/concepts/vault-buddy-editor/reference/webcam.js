/* Webcam is an independent source, not a baked-in screen effect.
   Camera permission is requested only after a deliberate click. No device ids
   or access grants are persisted with a project. */
const WEBCAM_MAX_MS = 180000;
const webcamState = {
  phase:'idle', stream:null, recorder:null, chunks:[], take:null, takeURL:null,
  generation:0, timer:0, countdown:0, startClock:0, duration_ms:0, bytes:0,
  insert_ms:0, closeRequested:false, saving:false, audioContext:null,
  audioNode:null, analyser:null, deviceListeners:[], ending:false, mic:false
};
Object.assign(ICONS, {
  webcam:'<path d="M23 7l-7 5 7 5V7Z"/><rect x="1" y="5" width="15" height="14" rx="3"/>',
  mirror:'<path d="M12 3v18M8 6 3 12l5 6V6Zm8 0 5 6-5 6V6Z"/>',
  circle:'<circle cx="12" cy="12" r="9"/>',
  stop:'<rect x="5" y="5" width="14" height="14" rx="2"/>'
});

function drawPresenter(g, ms=0) {
  g.fillStyle='#bca7bf';g.fillRect(0,0,BASE_W,BASE_H);
  box(g,110,45,340,420,12,'#ddd0d5');box(g,127,62,305,386,6,'#e9dee0');
  g.fillStyle='#cebbc7';g.fillRect(273,62,12,386);g.fillRect(127,254,305,12);
  box(g,955,260,132,285,12,'#7b6e7d');
  g.strokeStyle='#697c76';g.lineWidth=15;g.beginPath();g.moveTo(1022,410);g.lineTo(1022,140);g.stroke();
  g.fillStyle='#82928a';for(const [x,y,angle] of [[970,200,-.5],[1066,255,.5],[971,310,-.5]]){g.save();g.translate(x,y);g.rotate(angle);g.beginPath();g.ellipse(0,0,58,26,0,0,Math.PI*2);g.fill();g.restore();}
  const breathe=Math.sin(ms/1200)*3;
  g.fillStyle='#4b4560';g.beginPath();g.ellipse(652,697+breathe,277,248,0,0,Math.PI*2);g.fill();
  box(g,612,360+breathe,83,115,32,'#d9a890');
  g.fillStyle='#ddaf98';g.beginPath();g.ellipse(654,284+breathe,123,160,0,0,Math.PI*2);g.fill();
  g.fillStyle='#393342';g.beginPath();g.moveTo(531,282+breathe);g.bezierCurveTo(487,33,836,18,777,280+breathe);g.lineTo(752,224);g.lineTo(735,171);g.bezierCurveTo(641,211,599,158,563,225);g.closePath();g.fill();
  const blink = ms%4300>4120;
  for(const x of [610,701]){g.strokeStyle='#473947';g.lineWidth=5;g.lineCap='round';g.beginPath();g.moveTo(x-8,286+breathe);g.lineTo(x+8,286+breathe+(blink?0:1));g.stroke();}
  g.strokeStyle='#ae776d';g.lineWidth=5;g.beginPath();g.arc(656,332+breathe,25,.25,Math.PI-.25);g.stroke();
  g.strokeStyle='#a48caf';g.lineWidth=5;g.beginPath();g.moveTo(615,476);g.lineTo(649,518);g.lineTo(689,475);g.stroke();
  box(g,515,611,285,46,23,'#302938bb');text(g,'PRESENTER · DEMO',547,642,22,'#f8f1ff',650);
}

function drawVideoImage(g, source, sourceW, sourceH, c) {
  g.save();g.fillStyle='#17151f';g.fillRect(0,0,W,H);
  if(c.mirror){g.translate(W,0);g.scale(-1,1);}
  const destRatio=c.w*W/(c.h*H), imageRatio=sourceW/sourceH;
  if((c.fit||'contain')==='cover') {
    let sw=sourceW,sh=sourceH;
    if(imageRatio>destRatio)sw=sourceH*destRatio;else sh=sourceW/destRatio;
    const zoom=c.crop_zoom||1;sw/=zoom;sh/=zoom;
    const sx=clamp((c.crop_x??.5)*sourceW-sw/2,0,sourceW-sw),sy=clamp((c.crop_y??.5)*sourceH-sh/2,0,sourceH-sh);
    g.drawImage(source,sx,sy,sw,sh,0,0,W,H);
  } else {
    const ratio=Math.min(c.w*W/sourceW,c.h*H/sourceH),w=sourceW*ratio/(c.w*W)*W,h=sourceH*ratio/(c.h*H)*H;
    g.drawImage(source,(W-w)/2,(H-h)/2,w,h);
  }
  g.restore();
}
const presenterCanvas=document.createElement('canvas');presenterCanvas.width=W;presenterCanvas.height=H;
const presenterCtx=presenterCanvas.getContext('2d');
function drawSource(g,c) {
  const a=asset(c.asset_id),t=sourceTime(c);
  if(a.builtin==='screen')drawDemo(g,t);
  else if(a.builtin==='detail')drawDetail(g,t);
  else if(a.builtin==='presenter'){drawPresenter(presenterCtx,t);drawVideoImage(g,presenterCanvas,W,H,c);}
  else {
    const r=runtime(c);
    if(r&&r.el.readyState>=2&&r.el.videoWidth)drawVideoImage(g,r.el,r.el.videoWidth,r.el.videoHeight,c);
    else {g.fillStyle='#272330';g.fillRect(0,0,W,H);text(g,'Reconnect original media',340,330,30,'#d9cced',550);text(g,a.name.slice(0,70),340,375,18,'#a998ba');}
  }
}
function clipVideoFrame(g,c,x,y,w,h) {
  const shape=c.frame_shape||(c.w<.98?'rounded':'rectangle');
  if(shape==='circle'){g.beginPath();g.ellipse(x+w/2,y+h/2,w/2,h/2,0,0,Math.PI*2);g.clip();}
  else if(shape==='rounded'){rounded(g,x,y,w,h,Math.min(w,h)*.075);g.clip();}
}
function videoLayoutHTML(c) {
  const cam=asset(c.asset_id)?.capture_origin==='webcam'||asset(c.asset_id)?.builtin==='presenter';
  return inspectorSection(cam?'Webcam overlay':'Video layout',`
    <div class="preset-row"><button data-video-preset="full">Full frame</button><button data-video-preset="pip">Picture-in-picture</button></div>
    <div class="fieldpair">${field('Horizontal (%)','clipX',+(c.x*100).toFixed(1),0,100-c.w*100,.5)}${field('Vertical (%)','clipY',+(c.y*100).toFixed(1),0,100-c.h*100,.5)}</div>
    ${range('Size','clipScale',+(c.w*100).toFixed(1),10,Math.min(100,100*c.w/c.h),.5,'%')}
    <div class="corner-presets" aria-label="Place overlay in a corner">
      ${[['tl','Top left'],['tr','Top right'],['bl','Bottom left'],['br','Bottom right']].map(([k,n])=>`<button data-video-corner="${k}" title="${n}" aria-label="Place ${n.toLowerCase()}"><i class="corner-icon ${k}"></i><span>${n}</span></button>`).join('')}
    </div>
    <p class="field-help">Drag to move. Drag any corner to resize. Size keeps the frame’s proportions; these controls work without dragging.</p>
  `)+inspectorSection('Frame & crop',`
    <div class="preset-row shape-presets">${[['rounded','Rounded'],['circle','Circle'],['rectangle','Rectangle']].map(([k,n])=>`<button data-video-shape="${k}" class="${(c.frame_shape||'rounded')===k?'active':''}" aria-pressed="${(c.frame_shape||'rounded')===k}">${n}</button>`).join('')}</div>
    ${selectField('Image fitting','videoFit',c.fit||'contain',[['cover','Fill · crop to frame'],['contain','Fit · show entire image']])}
    ${(c.fit||'contain')==='cover'?`${range('Crop zoom','videoCropZoom',c.crop_zoom||1,1,3,.05,'×')}<div class="fieldpair">${field('Focus X (%)','videoCropX',(c.crop_x??.5)*100,0,100,1)}${field('Focus Y (%)','videoCropY',(c.crop_y??.5)*100,0,100,1)}</div>`:''}
    <label class="check-row"><input type="checkbox" data-prop="videoMirror" ${c.mirror?'checked':''}>Mirror this video</label>
    ${range('Opacity','opacity',c.opacity*100,0,100,1,'%')}
    <p class="field-help">Mirroring and crop affect preview and renders, never the original. ${cam?'The webcam stays on its own editable video track.':''}</p>
  `);
}
function sizeVideo(c, width) {
  const ratio=c.h/c.w;
  c.w=clamp(width,Math.max(.1,.1/ratio),Math.min(1,1/ratio));c.h=c.w*ratio;
  c.x=clamp(c.x,0,1-c.w);c.y=clamp(c.y,0,1-c.h);
}
function setVideoCorner(c,where) {const mx=.035,my=.05;c.x=where.endsWith('r')?Math.max(0,1-c.w-mx):Math.min(mx,1-c.w);c.y=where.startsWith('b')?Math.max(0,1-c.h-my):Math.min(my,1-c.h);}
function setVideoShape(c,shape) {
  const wasCircle=c.frame_shape==='circle';c.frame_shape=shape;
  if(shape==='circle'){c.h=c.w*W/H;c.fit='cover';}
  else if(wasCircle)c.h=c.w;
  sizeVideo(c,c.w);
}

function webcamSupportedMime() {
  if(typeof MediaRecorder==='undefined')return null;
  const hasMic=webcamState.stream?.getAudioTracks().length>0;
  const choices=hasMic?['video/webm;codecs=vp8,opus','video/webm;codecs=vp9,opus','video/webm','video/mp4']:['video/webm;codecs=vp8','video/webm;codecs=vp9','video/webm','video/mp4'];
  return choices.find(type=>MediaRecorder.isTypeSupported(type))||null;
}
function stopWebcamDevices() {
  for(const [track,fn] of webcamState.deviceListeners)track.removeEventListener('ended',fn);
  webcamState.deviceListeners=[];
  if(webcamState.stream)webcamState.stream.getTracks().forEach(t=>t.stop());
  webcamState.stream=null;
  webcamState.audioNode?.disconnect();webcamState.audioNode=null;
  webcamState.audioContext?.close().catch(()=>{});webcamState.audioContext=null;webcamState.analyser=null;
  const video=$('#webcamVideo');if(video?.srcObject)video.srcObject=null;
}
function webcamErrorMessage(error) {
  const names={NotAllowedError:'Camera access was denied. Allow camera access in the browser’s site settings, then try again. The microphone is optional.',NotFoundError:'No matching camera or microphone was found. Connect a device, or turn off the microphone and try again.',NotReadableError:'The camera or microphone is busy or unavailable. Close other apps using it and retry.',OverconstrainedError:'The selected device is no longer available. Choose the default device and retry.',SecurityError:'This page is not allowed to access the camera. Open the HTML directly in a desktop browser, or serve it on localhost / HTTPS.'};
  return names[error?.name]||error?.message||'The camera could not be started. Import an existing webcam recording instead.';
}
function setWebcamMessage(message,isError=false) {const e=$('#webcamMessage');e.textContent=message;e.classList.toggle('error-text',isError);}
function openWebcam() {
  if(ui.exporting||projectBusy)return;pause();webcamState.insert_ms=Math.round(ui.time);webcamState.closeRequested=false;
  $('#webcamStart').value=(webcamState.insert_ms/1000).toFixed(2);$('#webcamMirror').checked=false;
  $('#webcamDialog').showModal();renderWebcam();
}
function renderWebcam() {
  const s=webcamState,active=['preview','countdown','recording'].includes(s.phase),recording=s.phase==='recording',review=s.phase==='review';
  $('#webcamState').textContent=({idle:'Camera off',requesting:'Requesting access…',preview:'Live preview · not recording',countdown:'Get ready',recording:'Recording',processing:'Preparing take…',review:'Review take · camera off'})[s.phase]||'Camera off';
  $('#webcamState').classList.toggle('recording',recording);$('#webcamState').classList.toggle('live',active);
  $('#webcamEmpty').hidden=active||review||s.phase==='processing';$('#webcamVideo').hidden=!active&&!review;
  $('#webcamCountdown').hidden=s.phase!=='countdown';$('#webcamTimer').hidden=!recording&&!review;
  $('#webcamTimer').textContent=fmt(s.duration_ms,true);
  for(const el of $$('#webcamSettings select,#webcamSettings input'))el.disabled=['requesting','countdown','recording','processing'].includes(s.phase)||s.saving;
  for(const el of $$('#webcamCamera,#webcamMic,#webcamMicSelect'))if(review)el.disabled=true;
  $('#webcamMirror').disabled=s.saving; // Display-only; saved with the clip on Add.
  $('#webcamDevices').hidden=!(active||review);$('#webcamVideo').style.transform=$('#webcamMirror').checked?'scaleX(-1)':'';
  $('#webcamMicSelect').disabled=!$('#webcamMic').checked||['requesting','countdown','recording','processing'].includes(s.phase)||review;
  const footer=$('#webcamFooter');
  if(s.closeRequested&&review){footer.innerHTML='<span class="small muted grow">This take has not been added to your project.</span><button class="btn" data-cam="keepReview">Keep reviewing</button><button class="btn danger" data-cam="discardClose">Discard & close</button>';return;}
  if(s.phase==='idle')footer.innerHTML='<button class="btn" data-cam="demo">Try demo overlay</button><button class="primary" data-cam="enable">'+svg('webcam')+'Enable camera</button>';
  else if(s.phase==='requesting')footer.innerHTML='<span class="small muted grow">Use your browser’s permission prompt.</span><button class="btn" data-cam="close">Cancel request</button>';
  else if(s.phase==='preview')footer.innerHTML='<button class="btn" data-cam="close">Cancel</button><button class="primary record-btn" data-cam="record"><i class="record-dot"></i>Start recording</button>';
  else if(s.phase==='countdown')footer.innerHTML='<span class="small muted grow">Recording starts after the countdown.</span><button class="btn" data-cam="cancelCountdown">Cancel countdown</button>';
  else if(recording)footer.innerHTML='<span class="small muted grow">Local recording · maximum 3 minutes</span><button class="primary record-btn" data-cam="stop">'+svg('stop')+'Stop & review</button>';
  else if(review)footer.innerHTML='<button class="btn" data-cam="retake">Discard & retake</button><button class="btn" data-cam="raw">'+svg('download')+'Save raw take</button><button class="primary" data-cam="add" '+(s.saving?'disabled':'')+'>'+svg('layers')+(s.saving?'Adding…':'Add as overlay')+'</button>';
  else footer.innerHTML='<span class="small muted">Preparing the recorded source…</span>';
}
async function populateCameraDevices() {
  try {
    const devices=await navigator.mediaDevices.enumerateDevices();
    for(const [kind,selector,label] of [['videoinput','#webcamCamera','Camera'],['audioinput','#webcamMicSelect','Microphone']]) {
      const el=$(selector),chosen=el.value;el.innerHTML='<option value="">System default</option>'+devices.filter(d=>d.kind===kind&&d.deviceId).map((d,i)=>`<option value="${esc(d.deviceId)}">${esc(d.label||label+' '+(i+1))}</option>`).join('');
      if([...el.options].some(o=>o.value===chosen))el.value=chosen;
    }
  } catch { /* Labels are a convenience; the default device still works. */ }
}
async function enableWebcam() {
  const s=webcamState;if(['recording','countdown','processing'].includes(s.phase))return;
  if(!navigator.mediaDevices?.getUserMedia||!window.isSecureContext){setWebcamMessage('Camera access needs a secure browser context. Open this HTML directly in desktop Chrome or Edge; localhost or HTTPS also works. File previews and embedded viewers may block permission.',true);return;}
  if(typeof MediaRecorder==='undefined'){setWebcamMessage('This browser cannot record camera video. Import an existing webcam recording instead.',true);return;}
  const generation=++s.generation;stopWebcamDevices();clearInterval(s.timer);s.phase='requesting';setWebcamMessage('Camera access is requested only now. Nothing is uploaded.');renderWebcam();
  const cameraId=$('#webcamCamera').value,micId=$('#webcamMicSelect').value,wantsMic=$('#webcamMic').checked;
  try {
    const stream=await navigator.mediaDevices.getUserMedia({video:{width:{ideal:1280},height:{ideal:720},frameRate:{ideal:30},...(cameraId?{deviceId:{exact:cameraId}}:{facingMode:'user'})},audio:wantsMic?{echoCancellation:true,noiseSuppression:true,...(micId?{deviceId:{exact:micId}}:{})}:false});
    if(generation!==s.generation||!$('#webcamDialog').open){stream.getTracks().forEach(t=>t.stop());return;}
    s.stream=stream;s.mic=stream.getAudioTracks().length>0;s.phase='preview';s.duration_ms=0;s.ending=false;
    const video=$('#webcamVideo');video.pause();video.removeAttribute('src');video.srcObject=stream;video.controls=false;video.muted=true;await video.play();
    for(const t of stream.getTracks()) {
      const ended=()=>{if(s.phase==='recording'){setWebcamMessage('A capture device disconnected. The available part of your take is being kept.',true);stopWebcam();}else{stopWebcamDevices();s.phase='idle';setWebcamMessage('The camera or microphone disconnected. Reconnect it and try again.',true);renderWebcam();}};
      t.addEventListener('ended',ended);s.deviceListeners.push([t,ended]);
    }
    if(s.mic){try{const AC=window.AudioContext||window.webkitAudioContext;s.audioContext=new AC();await s.audioContext.resume();s.audioNode=s.audioContext.createMediaStreamSource(stream);s.analyser=s.audioContext.createAnalyser();s.analyser.fftSize=256;s.audioNode.connect(s.analyser);}catch{/* Recording works without a meter. */}}
    s.timer=setInterval(updateWebcamClock,120);await populateCameraDevices();
    setWebcamMessage(s.mic?'Microphone included. Live preview is muted to prevent feedback.':'Camera only. Enable the microphone to record narration with this take.');renderWebcam();
  } catch(error) {if(generation!==s.generation)return;stopWebcamDevices();s.phase='idle';setWebcamMessage(webcamErrorMessage(error),true);renderWebcam();}
}
function updateWebcamClock() {
  const s=webcamState;if(s.phase==='recording'){s.duration_ms=Math.round(performance.now()-s.startClock);$('#webcamTimer').textContent=fmt(s.duration_ms,true);if(s.duration_ms>=WEBCAM_MAX_MS)stopWebcam();}
  if(s.analyser){const data=new Float32Array(s.analyser.fftSize);s.analyser.getFloatTimeDomainData(data);let peak=0;for(const x of data)peak=Math.max(peak,Math.abs(x));$('#webcamMicMeter').style.width=clamp((20*Math.log10(peak||.000001)+60)/60*100,0,100)+'%';}
  else $('#webcamMicMeter').style.width='0%';
}
function countdownWebcam() {
  const s=webcamState;if(s.phase!=='preview'||!s.stream)return;
  const position=Number($('#webcamStart').value);if(!Number.isFinite(position)||position<0||position>7200){setWebcamMessage('Enter an insertion time between 0 and 7200 seconds.',true);return;}
  s.insert_ms=Math.round(position*1000);s.phase='countdown';s.countdown=3;$('#webcamCountdown').textContent=3;renderWebcam();
  clearInterval(s.timer);s.timer=setInterval(()=>{if(--s.countdown>0)$('#webcamCountdown').textContent=s.countdown;else{clearInterval(s.timer);startWebcamRecording();}},1000);
}
function startWebcamRecording() {
  const s=webcamState;if(!s.stream)return;s.chunks=[];s.bytes=0;s.duration_ms=0;s.ending=false;
  try {
    const mime=webcamSupportedMime();if(!mime)throw new Error('No supported webcam recording codec was found. Import a camera file instead.');
    s.recorder=new MediaRecorder(s.stream,{mimeType:mime,videoBitsPerSecond:4000000,audioBitsPerSecond:128000});
    s.recorder.ondataavailable=event=>{if(event.data.size){s.chunks.push(event.data);s.bytes+=event.data.size;if(s.bytes>180*1024*1024&&s.phase==='recording'){setWebcamMessage('The recording size limit was reached. Keeping this take.',true);stopWebcam();}}};
    s.recorder.onerror=()=>{setWebcamMessage('The camera encoder stopped unexpectedly. Keeping the available recording.',true);stopWebcam();};
    s.recorder.onstop=finishWebcamTake;s.startClock=performance.now();s.recorder.start(250);s.phase='recording';s.timer=setInterval(updateWebcamClock,120);setWebcamMessage('Recording camera'+(s.mic?' and microphone':'')+'. Your screen recording and webcam remain separate sources.');renderWebcam();
  } catch(error){s.phase='preview';setWebcamMessage(webcamErrorMessage(error),true);renderWebcam();}
}
function stopWebcam() {
  const s=webcamState;if(s.ending)return;s.ending=true;clearInterval(s.timer);s.duration_ms=Math.min(WEBCAM_MAX_MS,Math.round(performance.now()-s.startClock));s.phase='processing';renderWebcam();
  if(s.recorder&&s.recorder.state!=='inactive')s.recorder.stop();else finishWebcamTake();
}
async function finishWebcamTake() {
  const s=webcamState;if(s.phase!=='processing')s.phase='processing';clearInterval(s.timer);
  const settings=s.stream?.getVideoTracks()[0]?.getSettings()||{},mime=s.recorder?.mimeType||'video/webm';
  stopWebcamDevices();
  const raw=new Blob(s.chunks,{type:mime});s.chunks=[];s.recorder=null;
  if(raw.size<100||s.duration_ms<MIN){s.phase='idle';s.take=null;setWebcamMessage('The take was too short to edit. Record at least a fraction of a second, then stop.',true);renderWebcam();return;}
  // MediaRecorder WebM streams omit a finite duration on some browsers. Insert
  // duration metadata without re-encoding so project reopening and seeks work.
  const blob=mime.includes('webm')?await withWebmDuration(raw,s.duration_ms):raw;
  if(s.takeURL)URL.revokeObjectURL(s.takeURL);
  const ext=mime.includes('mp4')?'mp4':'webm',name='Webcam-'+new Date().toISOString().replace(/[:.]/g,'-')+'.'+ext;
  s.take={file:new File([blob],name,{type:mime,lastModified:Date.now()}),duration_ms:s.duration_ms,width:settings.width||1280,height:settings.height||720,has_audio:s.mic};
  s.takeURL=URL.createObjectURL(s.take.file);s.phase='review';s.closeRequested=false;
  const v=$('#webcamVideo');v.srcObject=null;v.src=s.takeURL;v.controls=true;v.muted=false;v.load();
  setWebcamMessage('Camera and microphone are off. Review this take, then add it as an independent overlay. Save the project to keep a durable copy.');renderWebcam();
}
/* Minimal EBML Duration insertion for browser WebM. Unknown-length Segment
   stays unknown; stored media bytes and timestamps are not re-encoded. */
async function withWebmDuration(blob,durationMs) {
  try {
    const b=new Uint8Array(await blob.arrayBuffer());
    const vint=(at,id=false)=>{if(at>=b.length||b[at]===0)return null;let n=1,mask=128;while(!(b[at]&mask)){mask>>=1;n++;if(n>8)return null;}if(at+n>b.length)return null;let value=id?b[at]:b[at]&(mask-1);for(let i=1;i<n;i++)value=value*256+b[at+i];return {n,value,unknown:!id&&Array.from(b.subarray(at,at+n)).every((v,i)=>v===(i===0?(mask*2-1):255))};};
    const element=at=>{const id=vint(at,true);if(!id)return null;const size=vint(at+id.n);if(!size)return null;return {id:id.value,start:at,data:at+id.n+size.n,end:at+id.n+size.n+size.value,sizeAt:at+id.n,size};};
    let segment=null,pos=0;while(pos<b.length){const e=element(pos);if(!e)break;if(e.id===0x18538067){segment=e;break;}pos=e.end;}
    if(!segment)return blob;
    let info=null;pos=segment.data;while(pos<b.length){const e=element(pos);if(!e)break;if(e.id===0x1549a966){info=e;break;}if(e.size.unknown)break;pos=e.end;}
    if(!info||info.end>b.length)return blob;
    let scale=1000000,existing=null;pos=info.data;while(pos<info.end){const e=element(pos);if(!e||e.end>info.end)break;if(e.id===0x2AD7B1){scale=0;for(const v of b.subarray(e.data,e.end))scale=scale*256+v;}if(e.id===0x4489)existing=e;pos=e.end;}
    const value=durationMs*1000000/scale;
    if(existing&&(existing.size.value===4||existing.size.value===8)){const view=new DataView(b.buffer);existing.size.value===8?view.setFloat64(existing.data,value):view.setFloat32(existing.data,value);return new Blob([b],{type:blob.type});}
    // Avoid shifting byte-based SeekHead/Cues indexes of an indexed file.
    pos=segment.data;while(pos<b.length){const e=element(pos);if(!e)break;if(e.id===0x114D9B74||e.id===0x1C53BB6B)return blob;if(e.size.unknown)break;pos=e.end;}
    const delta=11,newSize=info.size.value+delta,encode=(v,n)=>{if(v>=2**(7*n)-1)return null;const out=new Uint8Array(n);for(let i=n-1;i>=0;i--){out[i]=v%256;v=Math.floor(v/256);}out[0]|=1<<(8-n);return out;};
    const size=encode(newSize,info.size.n);if(!size)return blob;
    const next=new Uint8Array(b.length+delta);next.set(b.subarray(0,info.end));next.set(b.subarray(info.end),info.end+delta);next.set(size,info.sizeAt);next.set([0x44,0x89,0x88],info.end);new DataView(next.buffer).setFloat64(info.end+3,value);
    if(!segment.size.unknown){const segsize=encode(segment.size.value+delta,segment.size.n);if(!segsize)return blob;next.set(segsize,segment.sizeAt);}
    return new Blob([next],{type:blob.type});
  } catch {return blob;}
}
async function addWebcamTake() {
  const s=webcamState;if(!s.take||s.saving)return;s.saving=true;renderWebcam();
  try {
    const insertion=Number($('#webcamStart').value);if(!Number.isFinite(insertion)||insertion<0||insertion>7200)throw new Error('Enter a valid insertion time between 0 and 7200 seconds.');s.insert_ms=Math.round(insertion*1000);
    const f=s.take.file,a={id:uid('asset'),name:f.name,kind:'video',capture_origin:'webcam',duration_ms:s.take.duration_ms,width:s.take.width,height:s.take.height,has_audio:s.take.has_audio,size:f.size,last_modified:f.lastModified};
    await attachFile(a,f);let inserted=null;
    const ok=change(()=>{project.assets.push(a);const t={id:uid('track'),kind:'video',name:'Webcam '+(project.assets.filter(a=>a.capture_origin==='webcam').length),visible:true,muted:false,solo:false,locked:false,volume:1};project.tracks.unshift(t);inserted=newClip(uid('clip'),a.id,t.id,'Webcam take',s.insert_ms,0,a.duration_ms,{x:.675,y:.65,w:.29,h:.29,fit:'cover',frame_shape:'rounded',mirror:$('#webcamMirror').checked,crop_zoom:1,crop_x:.5,crop_y:.5,fade_in_ms:Math.min(350,a.duration_ms/2),fade_out_ms:Math.min(350,a.duration_ms/2)});setVideoCorner(inserted,'br');project.clips.push(inserted);ui.time=inserted.start_ms+Math.min(500,a.duration_ms/2);ui.selected={type:'clip',id:inserted.id};ui.propertyTab='layout';ui.tab='media';},'Add webcam overlay');
    if(!ok){media.delete(a.id);throw new Error('The take could not be added. Save the raw take or retry.');}
    cacheFile(a,f);s.take=null;closeWebcamNow();syncMedia(true);toast('Webcam added on its own track. Resize it in the preview; Save project keeps the original take.');
  } catch(e){setWebcamMessage(e.message,true);}finally{s.saving=false;if($('#webcamDialog').open)renderWebcam();}
}
function closeWebcamNow() {
  const s=webcamState;++s.generation;clearInterval(s.timer);stopWebcamDevices();s.phase='idle';s.ending=false;s.take=null;s.closeRequested=false;
  const video=$('#webcamVideo');video.pause();video.removeAttribute('src');video.srcObject=null;video.load();
  if(s.takeURL)URL.revokeObjectURL(s.takeURL);s.takeURL=null;$('#webcamDialog').close();
}
function requestCloseWebcam() {
  const s=webcamState;if(s.saving||s.phase==='processing')return;
  if(s.phase==='recording'){stopWebcam();return;}
  if(s.phase==='review'&&s.take){s.closeRequested=true;renderWebcam();return;}
  closeWebcamNow();
}
function addDemoWebcam() {
  closeWebcamNow();let a=project.assets.find(a=>a.builtin==='presenter');
  change(()=>{if(!a){a={id:uid('asset'),kind:'video',builtin:'presenter',name:'Presenter · demo',duration_ms:33500,width:1280,height:720};project.assets.push(a);}const t={id:uid('track'),kind:'video',name:'Webcam · demo',visible:true,muted:true,solo:false,locked:false,volume:1};project.tracks.unshift(t);const c=newClip(uid('clip'),a.id,t.id,'Presenter · demo',Math.round(ui.time),0,Math.min(8000,Math.max(1500,duration()-ui.time)),{x:.745,y:.055,w:.22,h:.22*W/H,frame_shape:'circle',fit:'cover',fade_in_ms:350,fade_out_ms:350});project.clips.push(c);ui.selected={type:'clip',id:c.id};ui.propertyTab='layout';ui.time=c.start_ms+500;},'Add sample webcam overlay');
  toast('Illustrated demo overlay added. Record webcam uses your real camera only after permission.');
}
function initWebcamUI() {
  $('#webcamDialog').addEventListener('cancel',ev=>{ev.preventDefault();requestCloseWebcam();});
  $('#webcamDialog').addEventListener('click',ev=>{if(ev.target===$('#webcamDialog')){const r=ev.target.getBoundingClientRect();if(ev.clientX<r.left||ev.clientX>r.right||ev.clientY<r.top||ev.clientY>r.bottom)requestCloseWebcam();}});
  for(const selector of ['#webcamCamera','#webcamMicSelect','#webcamMic'])$(selector).addEventListener('change',()=>{if(webcamState.phase==='preview')enableWebcam();});
  $('#webcamMirror').addEventListener('change',()=>{$('#webcamVideo').style.transform=$('#webcamMirror').checked?'scaleX(-1)':'';});
  document.addEventListener('visibilitychange',()=>{if(document.hidden&&['preview','requesting','countdown'].includes(webcamState.phase))closeWebcamNow();else if(document.hidden&&webcamState.phase==='recording'){setWebcamMessage('Recording stopped because this window was hidden. The available take is kept for review.',true);stopWebcam();}});
  window.addEventListener('pagehide',()=>{if(webcamState.recorder?.state==='recording')webcamState.recorder.stop();stopWebcamDevices();});
  Object.assign(window.VaultBuddyEditor,{openWebcam,getWebcamState:()=>({phase:webcamState.phase,duration_ms:webcamState.duration_ms,device_active:!!webcamState.stream,take_bytes:webcamState.take?.file?.size||0}),withWebmDuration});
}
document.addEventListener('click',ev=>{
  const b=ev.target.closest('button');if(!b)return;
  const d=b.dataset;
  if(d.action==='webcam'||d.cam||d.videoCorner||d.videoShape||d.videoPreset){
    ev.stopImmediatePropagation();if(ui.exporting||projectBusy)return;
    if(d.action==='webcam'){openWebcam();return;}
    if(d.cam){const s=webcamState;switch(d.cam){case'enable':enableWebcam();break;case'record':countdownWebcam();break;case'stop':stopWebcam();break;case'add':addWebcamTake();break;case'close':requestCloseWebcam();break;case'keepReview':s.closeRequested=false;renderWebcam();break;case'discardClose':closeWebcamNow();break;case'raw':if(s.take)downloadBlob(s.take.file,s.take.file.name);break;case'retake':{s.take=null;s.closeRequested=false;$('#webcamVideo').pause();if(s.takeURL)URL.revokeObjectURL(s.takeURL);s.takeURL=null;enableWebcam();break;}case'cancelCountdown':clearInterval(s.timer);s.phase='preview';s.timer=setInterval(updateWebcamClock,120);renderWebcam();break;case'demo':addDemoWebcam();break;}return;}
    const c=selectedClip();if(!editable(c,true))return;
    change(()=>{if(d.videoCorner)setVideoCorner(c,d.videoCorner);if(d.videoShape)setVideoShape(c,d.videoShape);if(d.videoPreset==='full'){c.x=c.y=0;c.w=c.h=1;c.frame_shape='rectangle';c.fit='contain';}else if(d.videoPreset==='pip'){c.w=c.h=.29;c.frame_shape='rounded';c.fit='cover';setVideoCorner(c,'br');}},'Change video framing');
  }
},true);

// Keep the preview usable after restoring a tall timeline on a short display.
requestAnimationFrame(()=>setTimelineHeight($('.timeline').getBoundingClientRect().height));
