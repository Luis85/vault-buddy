# Design-System Foundation + Top-Screen Consolidation Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the missing UI token layer + shared component primitives, then refactor the shell, home (VaultList), and tasks-view components onto them with a behavior-preserving consolidation refresh.

**Architecture:** Purely presentational/frontend. Add a Tailwind 4 `@theme` block of semantic tokens (each mapped to a current Tailwind palette value via `var(--color-…)`, so nothing renders differently), add nine small SFC primitives under `src/components/ui/`, then replace copy-pasted utility strings in the target screens with those primitives. No `src-tauri`, config, IPC, or store changes. The existing Vitest suite is the regression net proving behavior is preserved.

**Tech Stack:** Vue 3.5 (`<script setup lang="ts">`), Tailwind CSS 4 (`@tailwindcss/vite`, CSS-based config — no `tailwind.config.*`), Vitest 4 + `@vue/test-utils` 2 + happy-dom.

## Global Constraints

- **No `src-tauri`/config/IPC/store changes.** This increment is frontend-only. (Windows remains the shipping gate; the `frontend` CI job is what this must pass.)
- **Behavior-preserving.** Every existing `data-testid`, `aria-label`, prop, and emit on refactored components stays byte-identical. Existing tests must pass **unchanged** except the one deliberate, called-out update in Task 12.
- **Tokens equal current values.** Every `@theme` color token is defined as `var(--color-<tailwind-palette>)` so it resolves to exactly today's value. The white-opacity glass surfaces (`bg-white/5`, `bg-white/10`, `border-white/10`) stay as literal utilities (already consistent; tokenizing risks `color-mix` drift for no gain).
- **Primitives live in `src/components/ui/`.** New tests are flat in `tests/` (repo convention), kebab-named.
- **Scope is fixed.** Touch only: `style.css`, the nine new `ui/` primitives, `ActionPanel.vue`, `VaultList.vue`, `TaskRow.vue`, `TaskComposer.vue`, `TaskListPicker.vue`, `TaskViewControls.vue`, `TaskDragHandle.vue`, `TaskEditor.vue`, `TaskSectionMenu.vue`, plus docs/baselines. Do **not** touch the other ~18 components (settings tabs, `Recordings`, `Transcriptions`, `McpSettings`, `Search`, import views, `UpdateView`, `TranscriptionSummary`).
- **TDD + Conventional Commits.** Failing test first for new primitives; `feat(ui)`/`refactor(ui)`/`style(ui)`/`docs` scopes seen in history. End nothing with the model identifier.
- **Icons standardize on `AppIcon`** (`src/components/AppIcon.vue`: a 24×24 stroked-svg wrapper, `size` prop default 16, children via default slot).

---

## Task 1: Semantic token layer (`@theme`)

**Files:**
- Modify: `src/style.css` (add an `@theme` block after the `@import "tailwindcss";` line)

**Interfaces:**
- Produces (Tailwind-generated utilities used by every later task): `text-fg`, `text-fg-secondary`, `text-fg-muted`, `text-fg-subtle`; `bg-accent`, `bg-accent-strong`, `text-accent-fg`; `ring-focus`, `border-focus`, `bg-focus`; `bg-success`, `bg-danger`, `text-danger-fg`, `bg-recording`; `rounded-control`; `text-micro`.

- [ ] **Step 1: Add the `@theme` block**

In `src/style.css`, immediately after `@import "tailwindcss";`, insert:

```css
/* Semantic design tokens. Each maps to the palette value already in use, so
   converted and unconverted components render identically — the win is a
   consistent vocabulary and the end of the slate-400/500 + error-color drift.
   The white-opacity glass surfaces stay as literal utilities on purpose. */
@theme {
  /* Foreground text ladder — rule: muted = captions/values,
     subtle = section labels / placeholders / disabled hints. */
  --color-fg: var(--color-slate-100);
  --color-fg-secondary: var(--color-slate-300);
  --color-fg-muted: var(--color-slate-400);
  --color-fg-subtle: var(--color-slate-500);

  /* Accent (violet) */
  --color-accent: var(--color-violet-500);
  --color-accent-strong: var(--color-violet-600);
  --color-accent-fg: var(--color-violet-200);
  --color-focus: var(--color-violet-400);

  /* Status */
  --color-success: var(--color-emerald-400);
  --color-danger: var(--color-red-400);
  --color-danger-fg: var(--color-red-300);
  --color-recording: var(--color-red-500);

  /* Control radius = the lg the buttons/rows already use */
  --radius-control: var(--radius-lg);

  /* The one caption size that folds the ad-hoc text-[10px] usages
     (fixed-geometry count badges keep their literal text-[9px]). */
  --text-micro: 0.625rem;
}
```

- [ ] **Step 2: Verify Tailwind compiles the tokens**

Run: `npm run build`
Expected: PASS (vue-tsc typecheck + Vite/Tailwind build succeed; the new utilities generate without error).

- [ ] **Step 3: Commit**

```bash
git add src/style.css
git commit -m "feat(ui): add semantic design tokens (@theme)"
```

---

## Task 2: `IconButton` primitive

**Files:**
- Create: `src/components/ui/IconButton.vue`
- Test: `tests/icon-button.test.ts`

**Interfaces:**
- Produces: `IconButton` — props `{ label: string; title?: string; size?: "sm"|"md" (default "md"); variant?: "ghost"|"danger" (default "ghost"); disabled?: boolean }`, default slot = icon, emits `click(ev: MouseEvent)`.

- [ ] **Step 1: Write the failing test**

```ts
// tests/icon-button.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import IconButton from "../src/components/ui/IconButton.vue";

describe("IconButton", () => {
  it("renders the slot and uses label as aria-label + title fallback", () => {
    const w = mount(IconButton, { props: { label: "Search" }, slots: { default: "<svg/>" } });
    const btn = w.get("button");
    expect(btn.attributes("aria-label")).toBe("Search");
    expect(btn.attributes("title")).toBe("Search");
    expect(w.find("svg").exists()).toBe(true);
  });

  it("carries the shared focus ring", () => {
    const w = mount(IconButton, { props: { label: "X" } });
    expect(w.get("button").classes()).toContain("focus-visible:ring-focus");
  });

  it("emits click when enabled and is inert when disabled", async () => {
    const w = mount(IconButton, { props: { label: "X", disabled: true } });
    expect(w.get("button").attributes("disabled")).toBeDefined();
    const enabled = mount(IconButton, { props: { label: "X" } });
    await enabled.get("button").trigger("click");
    expect(enabled.emitted("click")).toHaveLength(1);
  });

  it("prefers an explicit title over the label", () => {
    const w = mount(IconButton, { props: { label: "Aria", title: "Tip" } });
    expect(w.get("button").attributes("title")).toBe("Tip");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/icon-button.test.ts`
Expected: FAIL (cannot resolve `../src/components/ui/IconButton.vue`).

- [ ] **Step 3: Write the primitive**

```vue
<!-- src/components/ui/IconButton.vue -->
<script setup lang="ts">
// Icon-only button. Encapsulates the hover/focus/disabled treatment that was
// copy-pasted 59× across the panel (header actions, VaultList row actions,
// TaskRow edit/archive), resolving their drift (slate-300 vs 400 base, white
// vs slate-100 hover, opacity-40 vs 50) into ONE treatment. The caller passes
// the icon via the default slot and a required accessible `label`.
withDefaults(
  defineProps<{
    label: string;
    title?: string;
    size?: "sm" | "md";
    variant?: "ghost" | "danger";
    disabled?: boolean;
  }>(),
  { size: "md", variant: "ghost", disabled: false },
);
defineEmits<{ (e: "click", ev: MouseEvent): void }>();
</script>

<template>
  <button
    type="button"
    :aria-label="label"
    :title="title ?? label"
    :disabled="disabled"
    class="shrink-0 cursor-pointer rounded-control text-fg-muted transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
    :class="[
      size === 'sm' ? 'p-1' : 'p-1.5',
      variant === 'danger' ? 'hover:text-danger-fg' : 'hover:text-fg',
    ]"
    @click="$emit('click', $event)"
  >
    <slot />
  </button>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/icon-button.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/IconButton.vue tests/icon-button.test.ts
git commit -m "feat(ui): add IconButton primitive"
```

---

## Task 3: `AppButton` primitive

**Files:**
- Create: `src/components/ui/AppButton.vue`
- Test: `tests/app-button.test.ts`

**Interfaces:**
- Produces: `AppButton` — props `{ variant?: "primary"|"secondary"|"ghost"|"danger" (default "primary"); size?: "sm"|"md" (default "md"); disabled?: boolean; type?: "button"|"submit" (default "button") }`, default slot = label, emits `click(ev: MouseEvent)`.

- [ ] **Step 1: Write the failing test**

```ts
// tests/app-button.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import AppButton from "../src/components/ui/AppButton.vue";

describe("AppButton", () => {
  it("renders label slot and defaults to a primary button", () => {
    const w = mount(AppButton, { slots: { default: "Save" } });
    expect(w.text()).toBe("Save");
    expect(w.get("button").attributes("type")).toBe("button");
    expect(w.get("button").classes()).toContain("bg-accent");
  });

  it("applies the secondary variant classes", () => {
    const w = mount(AppButton, { props: { variant: "secondary" }, slots: { default: "x" } });
    expect(w.get("button").classes()).toContain("bg-white/5");
  });

  it("emits click; disabled suppresses it", async () => {
    const w = mount(AppButton, { slots: { default: "x" } });
    await w.get("button").trigger("click");
    expect(w.emitted("click")).toHaveLength(1);
    const d = mount(AppButton, { props: { disabled: true }, slots: { default: "x" } });
    expect(d.get("button").attributes("disabled")).toBeDefined();
  });

  it("supports type=submit for form composers", () => {
    const w = mount(AppButton, { props: { type: "submit" }, slots: { default: "x" } });
    expect(w.get("button").attributes("type")).toBe("submit");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/app-button.test.ts`
Expected: FAIL (cannot resolve the SFC).

- [ ] **Step 3: Write the primitive**

```vue
<!-- src/components/ui/AppButton.vue -->
<script setup lang="ts">
// Text button with the shared variants. Primary = accent fill; secondary =
// bordered glass; ghost = text-only; danger = danger fill.
withDefaults(
  defineProps<{
    variant?: "primary" | "secondary" | "ghost" | "danger";
    size?: "sm" | "md";
    disabled?: boolean;
    type?: "button" | "submit";
  }>(),
  { variant: "primary", size: "md", disabled: false, type: "button" },
);
defineEmits<{ (e: "click", ev: MouseEvent): void }>();

const VARIANT: Record<string, string> = {
  primary: "bg-accent text-white hover:bg-accent-strong",
  secondary: "border border-white/10 bg-white/5 text-fg hover:bg-white/10",
  ghost: "text-fg-muted hover:bg-white/10 hover:text-fg",
  danger: "bg-danger text-white hover:opacity-90",
};
</script>

<template>
  <button
    :type="type"
    :disabled="disabled"
    class="cursor-pointer rounded-control font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
    :class="[size === 'sm' ? 'px-2 py-1 text-xs' : 'px-3 py-1.5 text-sm', VARIANT[variant]]"
    @click="$emit('click', $event)"
  >
    <slot />
  </button>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/app-button.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/AppButton.vue tests/app-button.test.ts
git commit -m "feat(ui): add AppButton primitive"
```

---

## Task 4: `Chip` primitive

**Files:**
- Create: `src/components/ui/Chip.vue`
- Test: `tests/chip.test.ts`

**Interfaces:**
- Produces: `Chip` — props `{ variant?: "neutral"|"accent"|"interactive" (default "neutral"); label?: string; title?: string }`, default slot = content, emits `click()` (only meaningful when `interactive`).

- [ ] **Step 1: Write the failing test**

```ts
// tests/chip.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import Chip from "../src/components/ui/Chip.vue";

describe("Chip", () => {
  it("renders a static span for neutral/accent", () => {
    const w = mount(Chip, { slots: { default: "3" } });
    expect(w.get("span").text()).toBe("3");
    expect(w.find("button").exists()).toBe(false);
  });

  it("renders an interactive button that emits click", async () => {
    const w = mount(Chip, { props: { variant: "interactive", label: "Filter by tag work" }, slots: { default: "#work" } });
    const btn = w.get("button");
    expect(btn.attributes("aria-label")).toBe("Filter by tag work");
    await btn.trigger("click");
    expect(w.emitted("click")).toHaveLength(1);
  });

  it("uses the micro type size", () => {
    const w = mount(Chip, { slots: { default: "x" } });
    expect(w.get("span").classes()).toContain("text-micro");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/chip.test.ts`
Expected: FAIL (cannot resolve the SFC).

- [ ] **Step 3: Write the primitive**

```vue
<!-- src/components/ui/Chip.vue -->
<script setup lang="ts">
// Pill for tags, counts, and filters. `interactive` renders a focusable
// button (the clickable tag chip); neutral/accent render a static span.
withDefaults(
  defineProps<{ variant?: "neutral" | "accent" | "interactive"; label?: string; title?: string }>(),
  { variant: "neutral" },
);
defineEmits<{ (e: "click"): void }>();
</script>

<template>
  <button
    v-if="variant === 'interactive'"
    type="button"
    :aria-label="label"
    :title="title"
    class="shrink-0 cursor-pointer rounded-full bg-white/10 px-1.5 text-micro text-accent-fg transition-colors hover:bg-accent/30 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    @click="$emit('click')"
  >
    <slot />
  </button>
  <span
    v-else
    :title="title"
    class="shrink-0 rounded-full px-1.5 text-micro"
    :class="variant === 'accent' ? 'bg-accent/20 text-accent-fg' : 'bg-white/10 text-fg-muted'"
  >
    <slot />
  </span>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/chip.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/Chip.vue tests/chip.test.ts
git commit -m "feat(ui): add Chip primitive"
```

---

## Task 5: `CountBadge` primitive

**Files:**
- Create: `src/components/ui/CountBadge.vue`
- Test: `tests/count-badge.test.ts`

**Interfaces:**
- Produces: `CountBadge` — props `{ count: number; max?: number (default 99) }`. Renders nothing when `count <= 0`; renders `max+` when `count > max`. Fixed geometry (`text-[9px]`, `min-w-[14px]`, `leading-[14px]`) — the caller positions it (e.g. `absolute -right-0.5 -top-0.5`).

- [ ] **Step 1: Write the failing test**

```ts
// tests/count-badge.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import CountBadge from "../src/components/ui/CountBadge.vue";

describe("CountBadge", () => {
  it("renders nothing for zero", () => {
    const w = mount(CountBadge, { props: { count: 0 } });
    expect(w.find("span").exists()).toBe(false);
  });

  it("renders the count", () => {
    const w = mount(CountBadge, { props: { count: 7 } });
    expect(w.get("span").text()).toBe("7");
  });

  it("caps at max with a plus (default 99)", () => {
    const w = mount(CountBadge, { props: { count: 250 } });
    expect(w.get("span").text()).toBe("99+");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/count-badge.test.ts`
Expected: FAIL (cannot resolve the SFC).

- [ ] **Step 3: Write the primitive**

```vue
<!-- src/components/ui/CountBadge.vue -->
<script setup lang="ts">
// The corner count badge (VaultList tasks button, ActionPanel all-tasks).
// Fixed geometry keeps a 14px circle legible, so it keeps its literal 9px
// text rather than the micro caption token. The caller owns positioning.
const props = withDefaults(defineProps<{ count: number; max?: number }>(), { max: 99 });
const label = () => (props.count > props.max ? `${props.max}+` : String(props.count));
</script>

<template>
  <span
    v-if="count > 0"
    class="min-w-[14px] rounded-full bg-accent px-1 text-center text-[9px] font-semibold leading-[14px] text-white"
  >{{ label() }}</span>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/count-badge.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/CountBadge.vue tests/count-badge.test.ts
git commit -m "feat(ui): add CountBadge primitive"
```

---

## Task 6: `StatusDot` primitive

**Files:**
- Create: `src/components/ui/StatusDot.vue`
- Test: `tests/status-dot.test.ts`

**Interfaces:**
- Produces: `StatusDot` — props `{ tone: "success"|"recording"|"transcribing"|"priority-high"|"priority-low"; pulse?: boolean (default false); title?: string }`. Renders `aria-hidden` span. Tone→class map uses literal palette colors (so VaultList's `bg-violet-400` transcribing-dot assertion stays green).

- [ ] **Step 1: Write the failing test**

```ts
// tests/status-dot.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import StatusDot from "../src/components/ui/StatusDot.vue";

describe("StatusDot", () => {
  it("maps each tone to its palette color", () => {
    const cases: Array<[string, string]> = [
      ["success", "bg-emerald-400"],
      ["recording", "bg-red-500"],
      ["transcribing", "bg-violet-400"],
      ["priority-high", "bg-red-400"],
      ["priority-low", "bg-slate-500"],
    ];
    for (const [tone, cls] of cases) {
      const w = mount(StatusDot, { props: { tone: tone as never } });
      expect(w.get("span").classes()).toContain(cls);
    }
  });

  it("adds animate-pulse only when pulsing and stays aria-hidden", () => {
    const on = mount(StatusDot, { props: { tone: "recording", pulse: true } });
    expect(on.get("span").classes()).toContain("animate-pulse");
    expect(on.get("span").attributes("aria-hidden")).toBe("true");
    const off = mount(StatusDot, { props: { tone: "success" } });
    expect(off.get("span").classes()).not.toContain("animate-pulse");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/status-dot.test.ts`
Expected: FAIL (cannot resolve the SFC).

- [ ] **Step 3: Write the primitive**

```vue
<!-- src/components/ui/StatusDot.vue -->
<script setup lang="ts">
// The 1.5px status dot (VaultList open/recording/transcribing, TaskRow
// priority). Tone colors are literal palette values on purpose: they are
// one-off status hues, and keeping them literal holds existing dot
// assertions (vault-list transcribing dot = bg-violet-400) green.
withDefaults(
  defineProps<{
    tone: "success" | "recording" | "transcribing" | "priority-high" | "priority-low";
    pulse?: boolean;
    title?: string;
  }>(),
  { pulse: false },
);
const TONE: Record<string, string> = {
  success: "bg-emerald-400",
  recording: "bg-red-500",
  transcribing: "bg-violet-400",
  "priority-high": "bg-red-400",
  "priority-low": "bg-slate-500",
};
</script>

<template>
  <span
    class="h-1.5 w-1.5 shrink-0 rounded-full"
    :class="[TONE[tone], pulse ? 'animate-pulse' : '']"
    :title="title"
    aria-hidden="true"
  />
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/status-dot.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/StatusDot.vue tests/status-dot.test.ts
git commit -m "feat(ui): add StatusDot primitive"
```

---

## Task 7: `Avatar` primitive

**Files:**
- Create: `src/components/ui/Avatar.vue`
- Test: `tests/avatar.test.ts`

**Interfaces:**
- Produces: `Avatar` — props `{ name: string; size?: "sm"|"md" (default "md") }`. Renders the uppercased first character in an accent-strong square. `sm` = `h-4 w-4 rounded text-[9px]` (TaskRow vault marker); `md` = `h-7 w-7 rounded-lg text-xs` (VaultList). `aria-hidden` (callers provide the accessible name on the surrounding control); a `title` attr falls through.

- [ ] **Step 1: Write the failing test**

```ts
// tests/avatar.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import Avatar from "../src/components/ui/Avatar.vue";

describe("Avatar", () => {
  it("shows the uppercased initial", () => {
    const w = mount(Avatar, { props: { name: "personal" } });
    expect(w.get("span").text()).toBe("P");
    expect(w.get("span").attributes("aria-hidden")).toBe("true");
  });

  it("sizes sm vs md", () => {
    const sm = mount(Avatar, { props: { name: "Work", size: "sm" } });
    expect(sm.get("span").classes()).toContain("h-4");
    const md = mount(Avatar, { props: { name: "Work" } });
    expect(md.get("span").classes()).toContain("h-7");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/avatar.test.ts`
Expected: FAIL (cannot resolve the SFC).

- [ ] **Step 3: Write the primitive**

```vue
<!-- src/components/ui/Avatar.vue -->
<script setup lang="ts">
// The letter avatar shared by VaultList (md) and TaskRow's vault marker (sm).
withDefaults(defineProps<{ name: string; size?: "sm" | "md" }>(), { size: "md" });
</script>

<template>
  <span
    class="flex shrink-0 items-center justify-center bg-accent-strong/80 font-bold text-white"
    :class="size === 'sm' ? 'h-4 w-4 rounded text-[9px]' : 'h-7 w-7 rounded-lg text-xs'"
    aria-hidden="true"
  >{{ name.charAt(0).toUpperCase() }}</span>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/avatar.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/Avatar.vue tests/avatar.test.ts
git commit -m "feat(ui): add Avatar primitive"
```

---

## Task 8: `SectionHeader` primitive

**Files:**
- Create: `src/components/ui/SectionHeader.vue`
- Test: `tests/section-header.test.ts`

**Interfaces:**
- Produces: `SectionHeader` — default slot = label; renders an `<h2>` with the uppercase tracking-wider subtle-text treatment (VaultList group headers, task section headers).

- [ ] **Step 1: Write the failing test**

```ts
// tests/section-header.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import SectionHeader from "../src/components/ui/SectionHeader.vue";

describe("SectionHeader", () => {
  it("renders an h2 with the subtle uppercase treatment", () => {
    const w = mount(SectionHeader, { slots: { default: "Favorites" } });
    const h = w.get("h2");
    expect(h.text()).toBe("Favorites");
    expect(h.classes()).toEqual(expect.arrayContaining(["uppercase", "text-fg-subtle"]));
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/section-header.test.ts`
Expected: FAIL (cannot resolve the SFC).

- [ ] **Step 3: Write the primitive**

```vue
<!-- src/components/ui/SectionHeader.vue -->
<script setup lang="ts">
// The uppercase group label shared by VaultList's favorites/open/other
// sections and the tasks-view section headers.
</script>

<template>
  <h2 class="mb-1 px-2 text-micro font-semibold uppercase tracking-wider text-fg-subtle">
    <slot />
  </h2>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/section-header.test.ts`
Expected: PASS (1 test).

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/SectionHeader.vue tests/section-header.test.ts
git commit -m "feat(ui): add SectionHeader primitive"
```

---

## Task 9: `Banner` primitive

**Files:**
- Create: `src/components/ui/Banner.vue`
- Test: `tests/banner.test.ts`

**Interfaces:**
- Produces: `Banner` — props `{ tone?: "danger"|"warning"|"info"|"success" (default "danger") }`, default slot = message. Renders a `<p>` alert strip. `danger` keeps the established banner colors (`bg-red-500/20 text-red-200`).

- [ ] **Step 1: Write the failing test**

```ts
// tests/banner.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import Banner from "../src/components/ui/Banner.vue";

describe("Banner", () => {
  it("defaults to the danger tone", () => {
    const w = mount(Banner, { slots: { default: "Boom" } });
    const p = w.get("p");
    expect(p.text()).toBe("Boom");
    expect(p.classes()).toEqual(expect.arrayContaining(["bg-red-500/20", "text-red-200"]));
  });

  it("switches tone classes", () => {
    const w = mount(Banner, { props: { tone: "success" }, slots: { default: "ok" } });
    expect(w.get("p").classes()).toContain("text-emerald-200");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/banner.test.ts`
Expected: FAIL (cannot resolve the SFC).

- [ ] **Step 3: Write the primitive**

```vue
<!-- src/components/ui/Banner.vue -->
<script setup lang="ts">
// Inline alert strip (the panel error banner, warnings). `danger` keeps the
// established banner hues (red-200 on red/20); callers own live regions.
withDefaults(defineProps<{ tone?: "danger" | "warning" | "info" | "success" }>(), { tone: "danger" });
const TONE: Record<string, string> = {
  danger: "bg-red-500/20 text-red-200",
  warning: "bg-amber-500/20 text-amber-200",
  info: "bg-white/5 text-fg-muted",
  success: "bg-emerald-500/20 text-emerald-200",
};
</script>

<template>
  <p class="rounded-control px-2 py-1 text-xs" :class="TONE[tone]">
    <slot />
  </p>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/banner.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/Banner.vue tests/banner.test.ts
git commit -m "feat(ui): add Banner primitive"
```

---

## Task 10: `Field` (text input) primitive

**Files:**
- Create: `src/components/ui/Field.vue`
- Test: `tests/field.test.ts`

**Interfaces:**
- Produces: `Field` — `v-model` (prop `modelValue: string`, emits `update:modelValue(v: string)`). Root element is the `<input>`, so `type`, `placeholder`, `aria-label`, and native listeners (`@keydown.escape`) fall through via default attr inheritance.

- [ ] **Step 1: Write the failing test**

```ts
// tests/field.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import Field from "../src/components/ui/Field.vue";

describe("Field", () => {
  it("reflects modelValue and emits update on input", async () => {
    const w = mount(Field, { props: { modelValue: "hi" } });
    const input = w.get("input");
    expect((input.element as HTMLInputElement).value).toBe("hi");
    await input.setValue("bye");
    expect(w.emitted("update:modelValue")).toEqual([["bye"]]);
  });

  it("passes through native attrs to the input root", () => {
    const w = mount(Field, {
      props: { modelValue: "" },
      attrs: { type: "search", placeholder: "Filter…", "aria-label": "Filter vaults" },
    });
    const input = w.get("input");
    expect(input.attributes("type")).toBe("search");
    expect(input.attributes("placeholder")).toBe("Filter…");
    expect(input.attributes("aria-label")).toBe("Filter vaults");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/field.test.ts`
Expected: FAIL (cannot resolve the SFC).

- [ ] **Step 3: Write the primitive**

```vue
<!-- src/components/ui/Field.vue -->
<script setup lang="ts">
// Standard text input. Root IS the <input>, so type/placeholder/aria-label
// and native listeners (@keydown.escape) fall through by default.
defineProps<{ modelValue: string }>();
defineEmits<{ (e: "update:modelValue", v: string): void }>();
</script>

<template>
  <input
    :value="modelValue"
    class="w-full rounded-control border border-white/10 bg-white/5 px-2 py-1 text-sm text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
    @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
  >
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/field.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/Field.vue tests/field.test.ts
git commit -m "feat(ui): add Field input primitive"
```

---

## Task 11: Refactor `ActionPanel` (shell header + error banner + filter)

**Files:**
- Modify: `src/components/ActionPanel.vue` (script imports + `<template>` header block lines ~159-248, error `<p>` lines ~249-254)
- Test (gate, unchanged): `tests/action-panel.test.ts`, `tests/recordings.test.ts`

**Interfaces:**
- Consumes: `IconButton`, `CountBadge`, `Chip`, `Banner`, `Field`.

- [ ] **Step 1: Establish the green baseline**

Run: `npx vitest run tests/action-panel.test.ts tests/recordings.test.ts`
Expected: PASS (record the counts; they must not change).

- [ ] **Step 2: Import the primitives**

In the `<script setup>` import block add:

```ts
import Banner from "./ui/Banner.vue";
import Chip from "./ui/Chip.vue";
import CountBadge from "./ui/CountBadge.vue";
import Field from "./ui/Field.vue";
import IconButton from "./ui/IconButton.vue";
```

- [ ] **Step 3: Replace the four header icon-buttons**

Replace the all-tasks button (lines ~166-186) with (keep the exact `data-testid`s and the aria-label expression):

```vue
<IconButton
  v-if="view === 'list' && store.vaults.length > 0"
  :label="`All tasks across every vault${totalOpenTasks > 0 ? ` — ${totalOpenTasks} open` : ''}`"
  title="All tasks"
  data-testid="all-tasks"
  class="relative"
  @click="store.openAllTasks()"
>
  <AppIcon>
    <path d="M9 11l3 3 8-8" />
    <path d="M20 12v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h9" />
  </AppIcon>
  <CountBadge
    :count="totalOpenTasks"
    data-testid="all-tasks-count"
    class="absolute -right-0.5 -top-0.5"
  />
</IconButton>
```

Replace the search button (lines ~187-204) and the settings button (lines ~205-224) with `IconButton` (keep `data-testid="search-toggle"` / `"settings-toggle"`, aria/title, and the `AppIcon` children). Replace the back button (lines ~225-237) with `IconButton label="Back" title="Back" data-testid="back-button" @click="store.back()"` wrapping its `AppIcon`. Delete the now-unused `taskBadge` computed only if nothing else uses it (CountBadge owns the cap) — grep first; keep it if referenced elsewhere.

- [ ] **Step 4: Replace the count pill and error banner**

The vault-count pill (lines ~160-165) becomes:

```vue
<Chip v-if="view === 'list' && store.vaults.length > 0">{{ store.vaults.length }}</Chip>
```

The error `<p>` (lines ~249-254) becomes:

```vue
<Banner v-if="view === 'list' && store.error" tone="danger" class="mb-2">{{ store.error }}</Banner>
```

The filter `<input>` (lines ~240-248) becomes `Field` (preserve the escape handler and aria-label):

```vue
<Field
  v-if="showFilter"
  v-model="filter"
  type="search"
  placeholder="Filter vaults…"
  aria-label="Filter vaults"
  class="mb-2"
  @keydown.escape="onFilterEscape"
/>
```

(Note: `Field`'s root input already carries the input styling; the `class="mb-2"` merges via attr fallthrough.)

- [ ] **Step 5: Run the gate + typecheck**

Run: `npx vitest run tests/action-panel.test.ts tests/recordings.test.ts && npm run build`
Expected: PASS with the SAME test counts as Step 1; build clean.

- [ ] **Step 6: Commit**

```bash
git add src/components/ActionPanel.vue
git commit -m "refactor(ui): put ActionPanel header/banner/filter on primitives"
```

---

## Task 12: Refactor `VaultList` (home)

**Files:**
- Modify: `src/components/VaultList.vue`
- Test (gate): `tests/vault-list.test.ts` — one deliberate assertion update (Step 5).

**Interfaces:**
- Consumes: `IconButton`, `StatusDot`, `Avatar`, `CountBadge`, `SectionHeader`.

- [ ] **Step 1: Establish the green baseline**

Run: `npx vitest run tests/vault-list.test.ts`
Expected: PASS (record the count).

- [ ] **Step 2: Import primitives and drop the local button-class helpers**

Add imports:

```ts
import Avatar from "./ui/Avatar.vue";
import CountBadge from "./ui/CountBadge.vue";
import IconButton from "./ui/IconButton.vue";
import SectionHeader from "./ui/SectionHeader.vue";
import StatusDot from "./ui/StatusDot.vue";
```

- [ ] **Step 3: Convert the structural bits**

- Group header `<h2>` (lines ~118-123) → `<SectionHeader v-if="group.label">{{ group.label }}</SectionHeader>`.
- The initial `<span>` (lines ~153-158) → `<Avatar :name="vault.name" size="md" />`.
- The three status dots (lines ~164-181) → `StatusDot`:

```vue
<StatusDot v-if="vault.open" tone="success" title="Open in Obsidian" />
<StatusDot v-if="vault.id === recordingVaultId" tone="recording" pulse title="Recording…" />
<StatusDot v-if="vault.id === transcribingVaultId" tone="transcribing" pulse title="Transcribing…" />
```

- The tasks count `<span>` (lines ~241-245) → `<CountBadge :count="taskCounts?.[vault.id] ?? 0" data-testid="task-count" class="absolute -right-0.5 -top-0.5" />`.

- [ ] **Step 4: Convert the four secondary action buttons to `IconButton`**

The daily-note, tasks, capture, and capture-settings buttons (lines ~197-298) each become an `IconButton` carrying the same `:disabled`, `:aria-label`, `title`, `@click`, and (tasks) `data-testid="open-tasks"` + the `class="relative"` for its badge. Keep each button's existing icon children (convert the inline daily-note and capture `<svg>` blocks to `<AppIcon>` with the same `<rect>`/`<path>` children, `AppIcon` default size 16). Example (daily note):

```vue
<IconButton
  :label="`Open today's daily note in ${accessibleName(vault)}`"
  title="Open today's daily note"
  :disabled="busyVaultId !== null"
  @click="$emit('open-daily-note', vault.id)"
>
  <span v-if="isBusy(vault, 'open_daily_note')" class="block h-4 w-4 animate-spin rounded-full border-2 border-white/30 border-t-white" role="status" aria-label="Opening daily note…" />
  <AppIcon v-else>
    <rect x="3" y="5" width="18" height="16" rx="2" />
    <path d="M8 3v4M16 3v4M3 11h18" />
  </AppIcon>
</IconButton>
```

For the favorite star (lines ~133-145): keep it a bespoke control (its color depends on `favoriteButtonClass` and it renders a text glyph), but route it through `IconButton` with `variant="ghost"` is optional — simpler to leave the star as-is this pass to avoid disturbing `aria-pressed`; standardize only its rounding to `rounded-control`. (Star stays; note it in the deferred list.) The main open button (lines ~146-196) stays a bespoke wide button — it is not an icon button.

- [ ] **Step 5: Update the one class-coupled test assertion**

`tests/vault-list.test.ts` line ~56 asserts the transcribing dot has `bg-violet-400`. `StatusDot tone="transcribing"` still applies exactly `bg-violet-400`, so the assertion holds **if** the selector still finds the dot. Run the file; if the dot selector changed, update the selector (not the asserted class) or switch the assertion to `title="Transcribing…"`:

```ts
// if needed, more robust than a color-class assertion:
expect(wrapper.get('[title="Transcribing…"]').exists()).toBe(true);
```

- [ ] **Step 6: Run the gate + typecheck**

Run: `npx vitest run tests/vault-list.test.ts && npm run build`
Expected: PASS (same count as Step 1, aside from the single intentional selector/assertion tweak if it was needed); build clean.

- [ ] **Step 7: Commit**

```bash
git add src/components/VaultList.vue tests/vault-list.test.ts
git commit -m "refactor(ui): put VaultList rows on primitives"
```

---

## Task 13: Refactor `TaskRow`

**Files:**
- Modify: `src/components/TaskRow.vue`
- Test (gate): `tests/tasks.test.ts`, `tests/task-reorder.test.ts`, `tests/task-editor.test.ts`

**Interfaces:**
- Consumes: `IconButton`, `Chip`, `StatusDot`, `Avatar`.

- [ ] **Step 1: Establish the green baseline**

Run: `npx vitest run tests/tasks.test.ts tests/task-reorder.test.ts tests/task-editor.test.ts`
Expected: PASS (record counts).

- [ ] **Step 2: Import primitives**

```ts
import Avatar from "./ui/Avatar.vue";
import Chip from "./ui/Chip.vue";
import IconButton from "./ui/IconButton.vue";
import StatusDot from "./ui/StatusDot.vue";
```

- [ ] **Step 3: Convert the row internals (preserve the `<li>` container state classes)**

- **Do NOT change** the `<li>` root (lines ~51-58): its `border-violet-400`/`border-white/10`/`opacity-50` drop-target/dragging classes are asserted by `task-reorder.test.ts` — leave them literal.
- Vault marker (lines ~92-97) → `<Avatar v-if="isAggregate" :name="task.vaultName" size="sm" :title="task.vaultName" data-testid="task-vault" />`.
- Priority dot (lines ~98-105) → `<StatusDot v-if="task.priority === 'high' || task.priority === 'low'" :tone="task.priority === 'high' ? 'priority-high' : 'priority-low'" :title="task.priority === 'high' ? 'High priority' : 'Low priority'" data-testid="task-priority" />`.
- Tag chips (lines ~113-123) → `Chip variant="interactive"` keeping `data-testid="task-tag"` and the click emit:

```vue
<Chip
  v-for="tag in task.tags"
  :key="tag"
  variant="interactive"
  :label="`Filter by tag ${tag}`"
  data-testid="task-tag"
  @click="$emit('tagClick', tag)"
>#{{ tag }}</Chip>
```

- Edit and archive buttons (lines ~131-183) → `IconButton size="sm"` keeping `data-testid="task-edit"`/`"task-archive"`, `:disabled="busy"`, aria/title, and their `AppIcon`-wrapped icons (move the inline `<svg>` pencil/box paths into `<AppIcon :size="14">`). Note `IconButton`'s disabled opacity is `50` (was `40` on the row) — an intentional consolidation; no test asserts `opacity-40`.

- [ ] **Step 4: Run the gate + typecheck**

Run: `npx vitest run tests/tasks.test.ts tests/task-reorder.test.ts tests/task-editor.test.ts && npm run build`
Expected: PASS with the SAME counts as Step 1; build clean.

- [ ] **Step 5: Commit**

```bash
git add src/components/TaskRow.vue
git commit -m "refactor(ui): put TaskRow chips/dots/actions on primitives"
```

---

## Task 14: Refactor `TaskComposer` + `TaskListPicker`

**Files:**
- Modify: `src/components/TaskComposer.vue`, `src/components/TaskListPicker.vue`
- Test (gate): `tests/tasks.test.ts`, `tests/tasks-lists.test.ts`

**Interfaces:**
- Consumes: `Field`, `AppButton`, `IconButton`.

- [ ] **Step 1: Establish the green baseline**

Run: `npx vitest run tests/tasks.test.ts tests/tasks-lists.test.ts`
Expected: PASS (record counts).

- [ ] **Step 2: Read both files first**

Run: open `src/components/TaskComposer.vue` and `src/components/TaskListPicker.vue`. Identify each `<input>`, each `<button>`, and any inline error `<p>`. (They are not shown here because the refactor is mechanical; apply the mapping below to each site you find.)

- [ ] **Step 3: Apply the primitive mapping**

- Every free-text `<input>` (title, tags, due — text ones) → `Field` with the same `v-model`, `type`, `placeholder`, `aria-label`, and listeners. Leave `<input type="date">` and `<select>`/`SelectMenu` as-is (Field is text-only).
- The submit/create button → `<AppButton variant="primary" type="submit">` (or `@click` if not a form) keeping its label, `:disabled`, and `data-testid`.
- Any secondary/cancel button → `<AppButton variant="secondary">` or `variant="ghost"`.
- Any icon-only control (e.g. the "New list…" confirm/cancel ✓/✕, an add glyph) → `IconButton` with its `aria-label`/`data-testid` preserved.
- Any inline error `<p>` → `<Banner tone="danger">` (import it) if one exists.

Preserve every `data-testid`, `aria-label`, `v-model`, emit, and IME/Enter/Escape handler exactly.

- [ ] **Step 4: Run the gate + typecheck**

Run: `npx vitest run tests/tasks.test.ts tests/tasks-lists.test.ts && npm run build`
Expected: PASS with the SAME counts as Step 1; build clean.

- [ ] **Step 5: Commit**

```bash
git add src/components/TaskComposer.vue src/components/TaskListPicker.vue
git commit -m "refactor(ui): put task composer + list picker on primitives"
```

---

## Task 15: Refactor `TaskViewControls`, `TaskEditor`, `TaskSectionMenu`, `TaskDragHandle`

**Files:**
- Modify: `src/components/TaskViewControls.vue`, `src/components/TaskEditor.vue`, `src/components/TaskSectionMenu.vue`, `src/components/TaskDragHandle.vue`
- Test (gate): `tests/tasks.test.ts`, `tests/tasks-lists.test.ts`, `tests/task-editor.test.ts`, `tests/task-sections.test.ts`, `tests/task-reorder.test.ts`

**Interfaces:**
- Consumes: `IconButton`, `AppButton`, `Field`, `SectionHeader`.

- [ ] **Step 1: Establish the green baseline**

Run: `npx vitest run tests/tasks.test.ts tests/tasks-lists.test.ts tests/task-editor.test.ts tests/task-sections.test.ts tests/task-reorder.test.ts`
Expected: PASS (record counts).

- [ ] **Step 2: Read the four files and map controls**

For each file, apply:
- Icon-only buttons (toolbar toggles' chevrons, the section-menu ⋯ trigger and its item rows, the drag grip's affordance, editor field controls) → `IconButton` (preserve `aria-label`/`aria-pressed`/`aria-expanded`/`data-testid` and all keydown handlers — the `TaskSectionMenu` Escape-swallowing and `TaskDragHandle` pointer/keydown behavior must be byte-identical).
- Text buttons (editor Save/Cancel, toolbar "＋ List" confirm) → `AppButton` (`primary` for Save/confirm, `ghost`/`secondary` for Cancel).
- Text `<input>`s (editor title/tags, the toolbar list-name input) → `Field`.
- Section-grouping segmented buttons (`Lists | Dates | Tags`): keep them as buttons but move their base color classes to tokens (`text-fg-muted`, `bg-white/10` active) — do **not** change their `aria-pressed`/behavior. (No `SegmentedControl` primitive this increment — YAGNI.)
- Any task section header text → `SectionHeader` **only if** it currently renders an `<h2>`; if it uses a different element/structure, leave the element and just swap its color/size classes to `text-fg-subtle`/`text-micro`, to avoid an a11y/DOM change the tests assert on.

- [ ] **Step 3: Run the gate + typecheck**

Run: `npx vitest run tests/tasks.test.ts tests/tasks-lists.test.ts tests/task-editor.test.ts tests/task-sections.test.ts tests/task-reorder.test.ts && npm run build`
Expected: PASS with the SAME counts as Step 1; build clean.

- [ ] **Step 4: Commit**

```bash
git add src/components/TaskViewControls.vue src/components/TaskEditor.vue src/components/TaskSectionMenu.vue src/components/TaskDragHandle.vue
git commit -m "refactor(ui): put remaining tasks-view controls on primitives"
```

---

## Task 16: (Optional) Empty-state polish + one cross-view fade

**Files:**
- Modify: `src/components/ActionPanel.vue` (the two bare `<p>` degraded states, lines ~385-397; optional `<Transition>` around the view `<div>` swap)
- Test (gate): `tests/action-panel.test.ts`

Only do this task if Tasks 11-15 are green and time allows; it is polish, not foundation.

- [ ] **Step 1: Baseline**

Run: `npx vitest run tests/action-panel.test.ts`
Expected: PASS.

- [ ] **Step 2: Consistent empty states**

Replace the "No vaults match…" and "Obsidian not found…" bare `<p>`s with a small shared block (icon + `text-fg-muted` message). Keep the exact text so the tests that assert on it stay green:

```vue
<div v-else-if="store.vaults.length > 0" class="px-2 py-6 text-center text-xs text-fg-muted">
  No vaults match "{{ filter }}".
</div>
<div v-else-if="store.loaded" class="px-2 py-6 text-center text-xs text-fg-muted">
  Obsidian not found — no vaults discovered. Is Obsidian installed and has it been opened at least once?
</div>
```

- [ ] **Step 3: (Optional) reduced-motion-safe cross-view fade**

Wrap the view-body `<div>`s' region in a `<Transition name="view" mode="out-in">` and add to `src/style.css`:

```css
.view-enter-active,
.view-leave-active { transition: opacity 120ms ease; }
.view-enter-from,
.view-leave-to { opacity: 0; }
```

The global `prefers-reduced-motion` rule in `style.css` already forces the duration to ~0, so this is safe. If `mode="out-in"` disturbs focus or the `panel-scroll` container in any test, **revert this step** — it is optional.

- [ ] **Step 4: Gate + build**

Run: `npx vitest run tests/action-panel.test.ts && npm run build`
Expected: PASS (same count).

- [ ] **Step 5: Commit**

```bash
git add src/components/ActionPanel.vue src/style.css
git commit -m "style(ui): consistent empty states + optional cross-view fade"
```

---

## Task 17: Docs, baselines, and the full quality gate

**Files:**
- Modify: `AGENTS.md` (Frontend state section — add a "UI primitives" subsection)
- Modify: `docs/Gaps.md` (add the deferred-work entries)
- Modify: `scripts/loc-baseline.json`, `scripts/quality-baseline.json` (via `--update`), and `vite.config.ts` only if a coverage floor rose

- [ ] **Step 1: Document the primitives in AGENTS.md**

Under the "Frontend state" section, add a subsection listing the token vocabulary (the `fg`/`accent`/status tokens, `rounded-control`, `text-micro`) and the nine primitives (`IconButton`, `AppButton`, `Chip`, `CountBadge`, `StatusDot`, `Avatar`, `SectionHeader`, `Banner`, `Field`) under `src/components/ui/`, with the rule: **new UI must use these instead of re-growing utility strings; the ~18 unconverted components migrate opportunistically.**

- [ ] **Step 2: Record deferred work in Gaps.md**

Add two Low entries: (a) density opportunities not taken (the list-view banner stack; VaultList's five-per-row actions + the still-bespoke favorite star) — the declutter theme; (b) the ~18 components still on raw utility classes, to be migrated onto the primitives in a follow-up.

- [ ] **Step 3: Run the full frontend gate**

Run: `npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage`
Expected: `lint` clean; `check:loc`/`check:quality` may report the baselines are now *loose* (metrics improved) — that is expected.

- [ ] **Step 4: Update the shrink-only baselines in the improving direction**

Run (only the ones the gate said improved):

```bash
node scripts/check-loc.mjs --update
node scripts/check-quality.mjs --update
```

Then re-run `npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage` and confirm all PASS. If a coverage floor in `vite.config.ts` rose, bump it to the new floor.

- [ ] **Step 5: Commit**

```bash
git add AGENTS.md docs/Gaps.md scripts/loc-baseline.json scripts/quality-baseline.json vite.config.ts
git commit -m "docs(ui): document primitives; record deferred UI migration; update baselines"
```

---

## Self-Review

**1. Spec coverage:**
- §2 token layer → Task 1. ✔ (fg ladder resolving slate-400/500, accent, status, `rounded-control`, `text-micro`; surfaces kept literal per the Global Constraints, matching the spec's "identity preserved").
- §3 primitives (9) → Tasks 2-10. ✔ Card/SurfaceSection + SegmentedControl were spec-optional and dropped (YAGNI; noted in Tasks 15/17).
- §4 VaultList → Task 12; Tasks view → Tasks 13-15. ✔
- §5 shell touch-ups → Task 11; optional fade + empty states → Task 16. ✔
- Testing (per-primitive + regression net) → each primitive task + the baseline/gate steps in 11-16. ✔
- Quality gates/docs/rollout → Task 17. ✔

**2. Placeholder scan:** Tasks 14-15 say "apply the mapping to each site you find" rather than reproducing every line of unread files — this is a deliberate mechanical mapping with the exact per-control rules and the existing test suite as the gate, not a "TBD". Every primitive task has complete code. No `TODO`/`TBD`/"handle edge cases".

**3. Type consistency:** Prop/emit names are consistent across tasks — `IconButton{label,title,size,variant,disabled}`+`click`; `AppButton{variant,size,disabled,type}`+`click`; `Chip{variant,label,title}`+`click`; `CountBadge{count,max}`; `StatusDot{tone,pulse,title}`; `Avatar{name,size}`; `Banner{tone}`; `Field{modelValue}`+`update:modelValue`. The tone strings used in Task 13 (`priority-high`/`priority-low`) match `StatusDot`'s definition in Task 6. The token utility names produced in Task 1 (`text-fg-muted`, `ring-focus`, `bg-accent`, `rounded-control`, `text-micro`) match every consumer.

**Risk note carried into execution:** Tasks 14-15 modify files not read while writing this plan (`TaskComposer`, `TaskListPicker`, `TaskViewControls`, `TaskEditor`, `TaskSectionMenu`, `TaskDragHandle`). Step 2 of each says read-first; the mapping rules + the per-file gate tests are the safety net. If a control's behavior can't be preserved by a straight primitive swap (e.g. a bespoke handler the primitive doesn't forward), leave that control as-is and note it — consolidation never trades behavior for tidiness.
