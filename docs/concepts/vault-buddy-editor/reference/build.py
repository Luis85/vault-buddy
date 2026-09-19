"""Build the standalone implementation reference. Python standard library only."""
from pathlib import Path
import argparse
JS_FILES=['drawing-primitives.js','editor.js','project-lifecycle.js','webcam.js','editing-features.js','captions.js','direct-edit.js','contextual-ui.js','preflight-recovery.js','onboarding.js','session-safety.js','workspace-ui.js','handover-polish.js']
CSS_FILES=['editor.css','onboarding.css','session-safety.css','workspace-ui.css','handover-polish.css']
def build(output: Path | None = None) -> Path:
    root=Path(__file__).resolve().parent
    js="'use strict';\n"+'\n'.join((root/f).read_text(encoding='utf-8') for f in JS_FILES)
    js+='\ninit().then(()=>{initWebcamUI();initFeatures();initPolish();Guide.init();Quality.init();FocusUI.init();HandoverUI.init();}).catch(HandoverUI.failed);'
    css='\n'.join((root/f).read_text(encoding='utf-8') for f in CSS_FILES)
    shell=(root/'shell.html').read_text(encoding='utf-8')
    if shell.count('/*STYLE*/')!=1 or shell.count('/*SCRIPT*/')!=1:raise ValueError('Missing or duplicate bundle anchors')
    destination=output or root.parent/'vault-buddy-editor.html'
    destination.parent.mkdir(parents=True,exist_ok=True)
    destination.write_text(shell.replace('/*STYLE*/',css).replace('/*SCRIPT*/',js),encoding='utf-8')
    return destination
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path)
    file=build(parser.parse_args().output);print(f'{file} ({file.stat().st_size:,} bytes)')
