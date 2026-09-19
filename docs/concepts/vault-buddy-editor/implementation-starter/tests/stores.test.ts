import { beforeEach, describe, expect, it } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { createEditorProjectStore } from '../src/useEditorProject';
import { useEditorWorkspace } from '../src/useEditorWorkspace';
import { createGuideStore } from '../src/useEditorOnboarding';
import { createListenerScope } from '../src/listenerScope';
import { decodeSnapshot, decodeProgress, type EditorPort, type EditorSnapshot } from '../src/contracts';
import { durationMs, sourceAt, frameTimestamp } from '../src/time';
const base: EditorSnapshot={sessionId:'session-a',projectId:'project-a',revision:1,persistedRevision:null,title:'Tutorial',durationMs:10000,canUndo:false,canRedo:false};
function deferred<T>() { let resolve!: (v:T)=>void; let reject!: (e:Error)=>void; const promise=new Promise<T>((a,b)=>{resolve=a;reject=b;});return {promise,resolve,reject}; }
function port(overrides: Partial<EditorPort>={}): EditorPort {
 return {openStaged:async()=>({...base}),execute:async request=>({...base,revision:request.expectedRevision+1,canUndo:true,title:request.command.kind==='rename'?request.command.title:'Tutorial'}),save:async(sessionId,revision)=>({sessionId,savedRevision:revision,projectFileId:'saved-project'}),...overrides};
}
beforeEach(()=>setActivePinia(createPinia()));
describe('project authority',()=>{
 it('opens a staged capture and starts dirty',async()=>{const s=createEditorProjectStore(port())();await s.openStaged('capture');expect(s.dirty).toBe(true);expect(s.busy).toBe(false);});
 it('applies only acknowledged edits',async()=>{const task=deferred<EditorSnapshot>();const s=createEditorProjectStore(port({execute:()=>task.promise}))();await s.openStaged('capture');const action=s.execute({kind:'rename',title:'Changed'});expect(s.snapshot?.title).toBe('Tutorial');task.resolve({...base,title:'Changed',revision:2});await action;expect(s.snapshot?.title).toBe('Changed');});
 it('marks saved only on matching native receipt',async()=>{const s=createEditorProjectStore(port())();await s.openStaged('capture');await s.save();expect(s.dirty).toBe(false);});
 it('rejects a mismatched save receipt without clearing dirty',async()=>{const s=createEditorProjectStore(port({save:async()=>({sessionId:'other',savedRevision:1,projectFileId:'p'})}))();await s.openStaged('capture');await expect(s.save()).rejects.toThrow();expect(s.dirty).toBe(true);expect(s.saving).toBe(false);});
 it('rejects stale revisions',async()=>{const s=createEditorProjectStore(port({execute:async()=>({...base,revision:0})}))();await s.openStaged('capture');await expect(s.execute({kind:'undo'})).rejects.toThrow('Stale');expect(s.snapshot?.revision).toBe(1);});
 it('does not install an open result after view disposal',async()=>{const task=deferred<EditorSnapshot>();const s=createEditorProjectStore(port({openStaged:()=>task.promise}))();const action=s.openStaged('capture');s.detach();task.resolve({...base});await action;expect(s.snapshot).toBe(null);});
 it('does not install an edit after disposal',async()=>{const task=deferred<EditorSnapshot>();const s=createEditorProjectStore(port({execute:()=>task.promise}))();await s.openStaged('capture');const action=s.execute({kind:'rename',title:'X'});s.detach();task.resolve({...base,revision:2});await action;expect(s.snapshot).toBe(null);});
 it('blocks double submissions',async()=>{const task=deferred<EditorSnapshot>();const s=createEditorProjectStore(port({execute:()=>task.promise}))();await s.openStaged('capture');const action=s.execute({kind:'undo'});await expect(s.execute({kind:'redo'})).rejects.toThrow('not ready');task.resolve({...base,revision:2});await action;});
 it('does not send empty title',async()=>{const s=createEditorProjectStore(port())();await s.openStaged('capture');await expect(s.execute({kind:'rename',title:' '})).rejects.toThrow();});
 it('keeps the existing project on open error',async()=>{const s=createEditorProjectStore(port({openStaged:async()=>{throw new Error('Missing capture');}}))();s.snapshot={...base};await expect(s.openStaged('capture')).rejects.toThrow();expect(s.snapshot?.projectId).toBe('project-a');});
});
describe('view and guidance isolation',()=>{
 it('seeking does not dirty the project',async()=>{const s=createEditorProjectStore(port())();await s.openStaged('capture');await s.save();const w=useEditorWorkspace();w.seek(15000,10000);expect(w.playheadMs).toBe(10000);expect(s.dirty).toBe(false);});
 it('deduplicates selection',()=>{const w=useEditorWorkspace();w.select(['a','a','b']);expect(w.selectedClipIds).toEqual(['a','b']);});
 it('resumes at the same step',()=>{const g=createGuideStore(['welcome','split','save'])();g.start();g.next();g.pause();g.start();expect(g.progress.stepId).toBe('split');});
 it('never auto-advances while suspended',()=>{const g=createGuideStore(['welcome','split'])();g.start();g.suspended=true;g.next();expect(g.progress.stepId).toBe('welcome');});
 it('cannot restore malformed completion',()=>{const g=createGuideStore(['welcome','split'])();expect(()=>g.restore({schema:'vault-buddy-guide-progress/1',stepId:'welcome',reviewed:[],status:'completed',dim:true})).toThrow();});
 it('releases late listener registration',async()=>{let stopped=0;const task=deferred<()=>void>();const scope=createListenerScope();const add=scope.add(task.promise);scope.dispose();task.resolve(()=>stopped++);await add;expect(stopped).toBe(1);});
 it('releases all listeners even if one cleanup throws',async()=>{let stopped=0;const s=createListenerScope();await s.add(Promise.resolve(()=>{throw new Error('Stop');}));await s.add(Promise.resolve(()=>stopped++));s.dispose();s.dispose();expect(stopped).toBe(1);});
});
describe('wire and time validation',()=>{
 it('rejects NaN, infinity and fractional revisions',()=>{for(const revision of [NaN,Infinity,1.2])expect(()=>decodeSnapshot({...base,revision})).toThrow();});
 it('rejects unrelated shape instead of trusting invoke generics',()=>expect(()=>decodeSnapshot('ok')).toThrow());
 it('rejects progress outside bounds',()=>expect(()=>decodeProgress({sessionId:'s',jobId:'j',sequence:1,phase:'rendering',fraction:2})).toThrow());
 it('maps a half-open source range at changed speed',()=>{const c={startMs:500,sourceInMs:1000,sourceOutMs:3000,speed:2};expect(durationMs(c)).toBe(1000);expect(sourceAt(c,500)).toBe(1000);expect(sourceAt(c,1499)).toBe(2998);expect(sourceAt(c,1500)).toBe(null);});
 it('derives fractional-rate timestamps without accumulation',()=>{expect(frameTimestamp(30000n,30000n,1001n)).toBe(10010000000n);expect(frameTimestamp(1n,30n,1n)).toBe(333333n);});
});
