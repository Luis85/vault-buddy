// TEST ONLY: managed browser policy disables physical camera, microphone and
// navigation. Use real canvas/audio MediaStreams in place of getUserMedia.
// The application code and MediaRecorder are NOT mocked.
Object.defineProperty(window,'isSecureContext',{value:true,configurable:true});
window.__cameraRequests=[];window.__testTracks=[];
Object.defineProperty(navigator,'mediaDevices',{value:{
 enumerateDevices:async()=>[{kind:'videoinput',deviceId:'simulated-camera',label:'Simulated camera (test only)'},{kind:'audioinput',deviceId:'simulated-mic',label:'Simulated microphone (test only)'}],
 getUserMedia:async constraints=>{
  window.__cameraRequests.push(constraints);
  const c=document.createElement('canvas');c.width=1280;c.height=720;const g=c.getContext('2d');
  const stream=c.captureStream(30);let count=0;
  const timer=setInterval(()=>{if(stream.getVideoTracks()[0].readyState==='ended'){clearInterval(timer);return;}drawPresenter(g,count++*33);},33);
  drawPresenter(g,0);
  if(constraints.audio){const ac=new AudioContext();await ac.resume();const o=ac.createOscillator(),gain=ac.createGain(),destination=ac.createMediaStreamDestination();o.frequency.value=660;gain.gain.value=.06;o.connect(gain);gain.connect(destination);o.start();const t=destination.stream.getAudioTracks()[0];stream.addTrack(t);const watchdog=setInterval(()=>{if(t.readyState==='ended'){clearInterval(watchdog);o.stop();ac.close();}},100);}
  window.__testTracks.push(...stream.getTracks());return stream;
 }
},configurable:true});
