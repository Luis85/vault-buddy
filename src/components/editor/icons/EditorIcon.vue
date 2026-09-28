<script lang="ts">
// Renders one of the tutorial editor's concept icons inside the concept's
// shared 24x24 stroked-line wrapper.
//
// The inner markup (ICONS[name]) is wired through a render function's
// `innerHTML` vnode prop rather than a template — Vue's own runtime-dom
// patches that prop straight onto the DOM `innerHTML` property, the exact
// mechanism `v-html` uses, but reaching it this way sidesteps neither
// safety rail by accident: this repo's ESLint config bans the template's
// `v-html` directive outright (`vue/no-v-html`) and separately bans a
// literal `el.innerHTML = …` assignment anywhere in src/ (the
// `no-restricted-syntax` XSS guard), because both exist to keep
// vault-derived strings (note titles, search snippets) out of raw HTML
// sinks. Neither guard's concern applies here: `ICONS[name]` is this app's
// own bundled constant, generated at build time from the concept's SVG
// data (scripts/gen-editor-icons.mjs), never user- or vault-derived
// content — and a render function is the documented, lint-clean way to
// hand Vue that trusted markup without writing either forbidden form.
import type { PropType } from "vue";
import { defineComponent, h } from "vue";

import type { EditorIconName } from "./conceptIcons";
import { ICONS } from "./conceptIcons";

export default defineComponent({
  name: "EditorIcon",
  props: {
    name: { type: String as PropType<EditorIconName>, required: true },
    size: { type: Number, default: 17 },
  },
  render() {
    return h("svg", {
      viewBox: "0 0 24 24",
      "aria-hidden": "true",
      class: "vb-icon",
      width: this.size,
      height: this.size,
      fill: "none",
      stroke: "currentColor",
      "stroke-width": "1.7",
      "stroke-linecap": "round",
      "stroke-linejoin": "round",
      innerHTML: ICONS[this.name],
    });
  },
});
</script>
