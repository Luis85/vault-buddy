# Design system and UI implementation rules

Use the existing Vault Buddy visual language: dark neutral surfaces, restrained violet accent, warm annotation/fade accents, small consistent radii and legible text. The supplied [design-tokens.json](../contracts/design-tokens.json) is extracted from the executable reference. Integrate semantic meanings into the repository's Tailwind/CSS tokens rather than importing a second framework or hardcoding every sample pixel.

## Layout and hierarchy

Desktop shell: application header, workspace with optional library/inspector, resizable timeline, unobtrusive status footer. The preview has one 48px control/header row. The media itself has a stable selected canvas ratio; never stretch it to fill arbitrary CSS space. Keep labels on primary actions and use tooltips/accessible names for secondary icon-only buttons. Avoid a permanent brand/version badge that competes with project work.

Persistent command ownership is intentional: project actions in the top header/menu; preview/teaching tools in the preview header; editing in the timeline/context menu; selected properties in the inspector; guidance in Help. Reuse one typed action registry for eligibility, label, shortcut, scope and execution. Overflow changes presentation, not behavior.

## Tokens and states

Use semantic foreground/background/border/accent/danger/success variables. Selected objects have both outline and descriptive state, not color alone. Text hierarchy distinguishes heading, body, helper and metadata; keep helpers readable at the compact desktop size. Never fade important warnings into decorative low-contrast text. Native high-contrast mode uses system colors and outlines where appropriate; media pixels themselves retain their authored color.

Buttons/fields have default, hover, focus-visible, pressed/selected, disabled, pending and error states. Disabled actions state the missing prerequisite, especially no selection/locked track/no media. Form errors are attached to their field and do not disappear into a short toast. Confirmation defaults protect data; destructive actions are not adjacent undifferentiated primary controls.

## Keyboard and focus

Toolbar: one tab stop with arrow/Home/End navigation, using proper toolbar semantics. Menus: named trigger, expanded state, menuitem roles, arrow navigation, disabled items and Escape focus return. Dialogs: labeled title, initial focus, focus containment, accessible close/cancel, Escape policy appropriate to active work, and return to opener or a logical replacement. A re-render must not drop the focused clip/control to body.

Shortcuts are scoped. Typing in fields, a guide, menu or dialog does not accidentally Split/Delete/Save the project. Selection nudges target the focused clip; transport keys target playback only when that context owns them. Every essential drag has a numeric/button/keyboard route. Do not claim full WCAG compliance from these design rules or screenshots; test with Windows keyboard and assistive technology.

## Motion and density

Guide highlights, panel changes and button feedback use short subdued motion. Honor `prefers-reduced-motion`; never make motion necessary to understand status. Keep canvas/video playback user-controlled and distinct from decorative animation. Compact layouts progressively disclose panels and secondary tools rather than wrapping repeated toolbar rows.

## Content rules

Use English consistently. Say **Save project** for editable workspace, **Render video** for a product, **Original media** for source recordings, **Reconnect** for unavailable files, and **Keep staged** for retained unsaved work. Say **Download started** in the browser path and **Saved** only for native confirmed persistence. Say **Privacy cover** and describe its limits; do not imply certified redaction. Avoid prototype/iteration labels in ordinary user navigation; the footer/help may accurately describe the browser execution boundary.

## Components to extract

Button/IconButton, Toolbar, MenuTrigger/ContextMenu, FormField, NumericSlider, Dialog, Toast/InlineAlert, StatusIndicator, TabList, ResizablePanel, AssetCard, TrackHeader, ClipItem/TrimHandle/FadeHandle, PreviewSelectionBox, GuideCoach/Highlight, EmptyState and ErrorState. Write Storybook-like component fixtures or the team's existing preview/test harness; adopting another catalog tool is optional, not an implicit dependency.

[WAI-ARIA toolbar and menu guidance](REFERENCES.md#official-guidance) informs the patterns. Validate behavior, not just role attributes.
