Feature: Vault Buddy tutorial creation and preservation
  The editable project remains the source of its rendered products.

  @A01
  Scenario: Correct native capture destination
    Given a finished capture is staged for vault A and the panel currently selects vault B
    When the editor opens the staged capture
    Then the native sidecar resolves vault A and no file is written to either vault before explicit save

  @A02
  Scenario: Stop still finalizing
    Given stop returns stillSaving and no terminal capture event has arrived
    When the UI reconciles capture status
    Then it remains in finalizing state and does not open an incomplete source as a completed recording

  @A03
  Scenario: Non-destructive cut and reopen
    Given a source file and an editable timeline with source-linked annotations exist
    When the user splits trims deletes saves and reopens the project
    Then source bytes are unchanged and retained clip/cue source intervals match the saved edit

  @A04
  Scenario: Atomic group movement
    Given selected clips on multiple tracks have relative offsets and one track is locked
    When the user moves the group
    Then the entire command is rejected or leaves all offsets unchanged without partially moving clips

  @A05
  Scenario: Global intro insertion
    Given a multi-track edit includes aligned screen webcam and narration
    When the user inserts an intro before everything
    Then all affected tracks shift by the same duration or the operation is refused atomically for locked content

  @A06
  Scenario: Monitoring is not export mute
    Given two audible tracks are included in the export mix
    When the user mutes monitoring and renders a short range
    Then decoded output still contains both source contributions

  @A07
  Scenario: Fades and crossfades
    Given compatible adjacent clips have an explicit transition and configured edge fades
    When the user plays renders saves and reopens
    Then the selected envelopes and transition timing are preserved and decoded output matches them

  @A08
  Scenario: Independent webcam source
    Given camera permission has not been granted and an existing project has unsaved work
    When the user opens webcam setup then declines permission
    Then no camera starts and the project remains unchanged and usable

  @A09
  Scenario: Webcam take retained
    Given the user recorded a webcam take but has not added or discarded it
    When the editor is closed or a new project is requested
    Then the pending take is included in the save/keep/discard decision and no media is silently lost

  @A10
  Scenario: Resizing survives serialization
    Given a circular presenter is placed over a screen recording
    When the user resizes and repositions it saves and reopens
    Then normalized layout and crop survive and the circle remains circular in output pixels

  @A11
  Scenario: Source-linked teaching cues
    Given an arrow text highlight zoom and step cue belong to a clip
    When the clip is moved split trimmed or changed in speed
    Then each cue follows the retained source interval and no dangling cue or duplicate boundary frame is introduced

  @A12
  Scenario: Caption timing and correction
    Given imported captions contain a timing overlap and a wrong word
    When the user corrects text and timing then exports
    Then the corrected synchronized cues are used and the overlap warning disappears only when resolved

  @A13
  Scenario: Output-range timestamps
    Given a project contains captions and chapters before and within a chosen nonzero range
    When the range is rendered
    Then caption/chapter timestamps are clipped and rebased to output zero while the project remains untrimmed

  @A14
  Scenario: Context target precision
    Given the playhead and pointer are at different valid times on a clip
    When the user invokes Split from that clip right-click context
    Then the command acts at the explicitly displayed context time rather than silently using stale selection or playhead

  @A15
  Scenario: Save without render
    Given the project contains imported originals but has no rendered products
    When the user saves a portable project and reopens it on a fresh session
    Then the composition is editable and available source media is restored without rendering first

  @A16
  Scenario: Immutable product lineage
    Given a product has been rendered from revision R
    When the user changes layout/color and renders another product
    Then the first encoded binary snapshot and source revision remain unchanged

  @A17
  Scenario: Historical source dependency
    Given a retained product snapshot uses a source removed from the current edit
    When the user saves a portable project
    Then that source remains included or is explicitly reported unavailable rather than silently pruned

  @A18
  Scenario: Matching durable receipt
    Given a save request targets revision R and a later current revision exists
    When the save receipt for R arrives
    Then the later revision remains dirty and no wrong-session receipt is applied

  @A19
  Scenario: Safe missing-media reconnection
    Given an original is unavailable and two selected candidates share its name
    When batch reconnect attempts to resolve media
    Then the UI reports ambiguity and preserves the edit until an explicit valid match is chosen

  @A20
  Scenario: Save fault recovery
    Given a valid project file exists
    When disk full or process interruption occurs during a replacement save
    Then the last good project and originals remain available and recovery reports the incomplete owned operation

  @A21
  Scenario: Partial output publication
    Given a rendered output is published but companion-note or product-record commit fails
    When the application restarts recovery
    Then the operation is resolved idempotently without overwriting unrelated files or falsely reporting full completion

  @A22
  Scenario: Malicious project rejection
    Given a project archive contains traversal duplicate IDs cyclic sources or unsafe expanded size
    When the user opens it
    Then validation rejects before installing state or granting filesystem access and current work remains untouched

  @A23
  Scenario: Read-only walkthrough
    Given a project has known composition and Undo state
    When the user reads all 22 lessons without choosing edit actions
    Then composition and Undo are unchanged and no camera download render or save action starts

  @A24
  Scenario: Exact guide resume
    Given the guide is paused or dismissed on a known stable step
    When the user reopens Help and resumes after native reload
    Then the same lesson and reviewed status are restored independently of the editing project

  @A25
  Scenario: Guide suspension
    Given the guide highlights a control that opens a modal dialog
    When the user opens and closes the dialog
    Then the coach suspends while modal and returns to the same step with logical keyboard focus

  @A26
  Scenario: No guide in render
    Given a guide highlight is visible over an editor control
    When a permitted rendering workflow is executed
    Then decoded video contains only authored composition and never guide outlines selection handles or UI

  @A27
  Scenario: Startup error clarity
    Given editor initialization fails before ready
    When the failure is displayed
    Then safe actionable recovery explains that saved project files were not changed without injecting exception text as markup

  @A28
  Scenario: No repeated toolbar work
    Given canvas dimensions remain unchanged across playback frames
    When the renderer checks canvas size repeatedly
    Then the aspect-ratio toolbar DOM is not rebuilt while real canvas changes update its label once

  @A29
  Scenario: Native temporal output
    Given moving numbered footage and synchronized audio pulses are rendered on supported hardware
    When the complete file is decoded
    Then frame coverage duration cadence and A/V offset satisfy recorded product tolerances rather than only file-existence checks

  @A30
  Scenario: Privacy boundary
    Given a static privacy cover hides sensitive output pixels
    When the user prepares to share an editable portable project
    Then the UI explains that uncensored originals can be included and does not claim secure source redaction

